"""NPU gateway: fair, single-slot queue in front of the cm-npu worker (Windows).

The worker (npu/worker/cm_npu_worker.py) runs OpenVINO models on the Intel NPU for
scientific computing: a request carries an OpenVINO IR model + input tensors (.npz)
and gets the output tensors back. Callers are identified by SO_PEERCRED; exactly one
request runs at a time, round-robin across users, FIFO within a user. Only this daemon
holds the worker token.
"""
import asyncio
import collections
import os
import time

import httpx

from .credentials import safe_mode

WORKER_URL = os.getenv("CM_NPU_URL", "http://127.0.0.1:8102")
KEY_FILE = os.getenv("CM_NPU_TOKEN_FILE", "/etc/compute-mgr/npu.token")
MAX_BODY = int(os.getenv("CM_NPU_MAX_BODY", str(256 << 20)))
MAX_QUEUED_PER_USER = int(os.getenv("CM_NPU_MAX_QUEUED", "4"))
WAIT_TIMEOUT = float(os.getenv("CM_NPU_WAIT_TIMEOUT", "300"))
RUN_TIMEOUT = float(os.getenv("CM_NPU_RUN_TIMEOUT", "600"))


class MissingToken(Exception):
    pass


class QueueFull(Exception):
    pass


class WaitTimeout(Exception):
    pass


class Scheduler:
    def __init__(self):
        self.waiting = collections.OrderedDict()   # user -> deque[Future]
        self.active = None                          # {"user","endpoint","model","started"}
        self.last_user = None
        self.stats = collections.Counter()
        self.latency = collections.deque(maxlen=50)

    def queued(self):
        return {u: len(q) for u, q in self.waiting.items() if q}

    def _grant_next(self):
        if self.active is not None:
            return
        order = list(self.waiting)            # every user ever seen, in arrival order
        if self.last_user in order:
            i = order.index(self.last_user)
            order = order[i + 1:] + order[:i + 1]
        users = [u for u in order if self.waiting[u]]
        if not users:
            return
        user = users[0]
        fut, info = self.waiting[user].popleft()
        self.last_user = user
        if not fut.done():
            # mark the slot taken *now*, before the waiter resumes, so no second grant can race in
            self.active = {**info, "user": user, "started": time.time()}
            fut.set_result(True)
        else:
            self._grant_next()

    async def acquire(self, user, info):
        q = self.waiting.setdefault(user, collections.deque())
        if len(q) >= MAX_QUEUED_PER_USER:
            raise QueueFull()
        fut = asyncio.get_running_loop().create_future()
        entry = (fut, info)
        q.append(entry)
        self._grant_next()
        try:
            async with asyncio.timeout(WAIT_TIMEOUT):
                await asyncio.shield(fut)
        except (TimeoutError, asyncio.CancelledError) as exc:
            if fut.done():
                self.release(False)
            else:
                q.remove(entry)
                fut.cancel()
            if isinstance(exc, TimeoutError):
                raise WaitTimeout() from exc
            raise

    def release(self, ok):
        if self.active:
            self.latency.append(time.time() - self.active["started"])
        self.stats["ok" if ok else "error"] += 1
        self.active = None
        self._grant_next()


class Gateway:
    def __init__(self):
        self.sched = Scheduler()
        self.client = None
        self._key = None

    def key(self):
        try:
            with open(KEY_FILE) as f:
                if safe_mode(KEY_FILE, os.fstat(f.fileno()).st_mode):
                    if not self._key:
                        self._key = f.read().strip()
                    return self._key
        except OSError:
            pass
        self._key = ""
        return self._key

    async def start(self):
        self.client = httpx.AsyncClient(base_url=WORKER_URL, timeout=httpx.Timeout(RUN_TIMEOUT, connect=5))

    async def stop(self):
        if self.client:
            await self.client.aclose()

    def headers(self):
        key = self.key()
        if not key:
            raise MissingToken()
        return {"Authorization": f"Bearer {key}"}

    async def status(self):
        worker = None
        try:
            r = await self.client.get("/status", headers=self.headers(), timeout=5)
            if r.status_code == 200:
                worker = r.json()
        except (httpx.HTTPError, ValueError, MissingToken):
            pass
        npu = (worker or {}).get("devices", {}).get("NPU")
        lat = list(self.sched.latency)
        return {"backend": WORKER_URL, "up": worker is not None, "npu": npu,
                "devices": sorted((worker or {}).get("devices", {})),
                "openvino": (worker or {}).get("openvino"), "cached_models": (worker or {}).get("cached_models"),
                "active": self.sched.active, "queued": self.sched.queued(),
                "served": self.sched.stats["ok"], "errors": self.sched.stats["error"],
                "avg_latency_s": round(sum(lat) / len(lat), 3) if lat else None}

    async def run(self, body):
        r = await self.client.post("/run", content=body, headers={**self.headers(), "Content-Type": "application/x-npz"})
        return r
