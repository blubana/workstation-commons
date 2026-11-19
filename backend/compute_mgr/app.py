from typing import Literal
import asyncio
import json
import os
import threading
import time
from contextlib import asynccontextmanager
from dataclasses import asdict
from io import BytesIO
from typing import Annotated

from fastapi import Depends, FastAPI, HTTPException, Query, Request, Response
from fastapi.exceptions import RequestValidationError
from fastapi.responses import JSONResponse, StreamingResponse
import anyio
import httpx
from pydantic import BaseModel, StrictInt
from starlette.concurrency import run_in_threadpool
from starlette.requests import ClientDisconnect
from starlette.exceptions import HTTPException as StarletteHTTPException

from . import helper, host, identity, npu, winhost
from .auth import Auth
from .collect import Collector
from .history import INTERVAL, RETENTION, History, Sampler


class Key(BaseModel):
    pubkey: str


class Role(BaseModel):
    role: Literal["admin", "user"]


class NewUser(Key):
    user: str


class WslConfig(BaseModel):
    memory_gb: StrictInt
    processors: StrictInt
    swap_gb: StrictInt


class Policy(BaseModel):
    mem_low_gb: StrictInt
    mem_high_gb: StrictInt
    total_max_gb: StrictInt
    cpu_weight: StrictInt


USER_METRICS = {"user_mem", "user_cpu"}
SYSTEM_METRICS = {
    "cpu_percent", "mem_used", "gpu_mem_used", "gpu_util", "gpu_temp",
    "net_rx", "net_tx", "disk_read", "disk_write",
}
ERROR_CODES = {400: "validation", 401: "unauthorized", 403: "forbidden",
               404: "not_found", 405: "method_not_allowed", 422: "validation"}


def create_app(*, collector=None, history=None, resolve_uid=identity.resolve_uid, sample=True):
    collector = collector if collector is not None else Collector()
    history = history if history is not None else History(os.getenv("CM_DB", "/var/lib/compute-mgr/metrics.db"))
    auth = Auth(resolve_uid)
    gateway = npu.Gateway()
    snapshot_lock = threading.Lock()
    snapshot = None
    last_snapshot = float("-inf")
    stream_counts = {}
    key_counts = {}
    key_lock = threading.Lock()

    def collect():
        nonlocal snapshot, last_snapshot
        with snapshot_lock:
            if time.monotonic() - last_snapshot >= 2:
                snapshot = collector.collect()
                last_snapshot = time.monotonic()
            return snapshot

    @asynccontextmanager
    async def lifespan(app):
        await run_in_threadpool(history.initialize)
        await gateway.start()
        sampler = Sampler(collect, history) if sample else None
        if sampler:
            sampler.thread.start()
        try:
            yield
        finally:
            if sampler:
                sampler.stop.set()
                await run_in_threadpool(sampler.thread.join)
            await gateway.stop()

    app = FastAPI(lifespan=lifespan, docs_url=None, redoc_url=None, openapi_url=None)
    Principal = Annotated[identity.Identity, Depends(auth.dependency())]
    Administrator = Annotated[identity.Identity, Depends(auth.dependency(admin=True))]

    @app.exception_handler(StarletteHTTPException)
    async def http_error(request, exc):
        return JSONResponse({"error": ERROR_CODES.get(exc.status_code, "failed"), "detail": exc.detail},
                            status_code=exc.status_code, headers=exc.headers)

    @app.exception_handler(RequestValidationError)
    async def validation_error(request, exc):
        details = [".".join(map(str, item["loc"])) + " " + item["msg"] for item in exc.errors()]
        return JSONResponse({"error": "validation", "detail": "; ".join(details)}, status_code=422)

    @app.exception_handler(helper.HelperError)
    async def helper_error(request, exc):
        return JSONResponse({"error": exc.code, "detail": exc.detail}, status_code=exc.status)

    @app.exception_handler(Exception)
    async def server_error(request, exc):
        return JSONResponse({"error": "failed", "detail": "Internal server error"}, status_code=500)

    def scoped(items, actor):
        return [item for item in items if actor.can_access(item["user"])]

    def users_for(users, actor):
        result = []
        for user in scoped(users, actor):
            name = user["user"]
            with key_lock:
                expires, count = key_counts.get(name, (0, None))
                if expires <= time.monotonic():
                    try:
                        count = len(helper.call("key-list", {"user": name}, actor.user)["keys"])
                    except helper.HelperError:
                        count = None
                    key_counts[name] = time.monotonic() + 30, count
            result.append({**user, "keys": count})
        return result

    def user_mutation(op, args, actor):
        response = helper.call(op, args, actor.user)
        with key_lock:
            key_counts.pop(args["user"], None)
        return response

    @app.get("/api/v1/auth/me")
    def me(actor: Principal):
        return asdict(actor)

    @app.get("/api/v1/system")
    def system(actor: Principal):
        return collect()[0]

    @app.get("/api/v1/users")
    def users(actor: Principal):
        return users_for(collect()[1], actor)

    @app.get("/api/v1/kernels")
    def kernels(actor: Principal):
        return scoped(collect()[2], actor)

    @app.delete("/api/v1/kernels/{id}", status_code=204)
    def stop_kernel(id: str, actor: Principal):
        kernel = next((item for item in scoped(collector.collect()[2], actor) if item["id"] == id), None)
        if kernel is None:
            raise HTTPException(404, "Kernel not found")
        helper.call("kernel-stop", {"pid": kernel["pid"], "owner": kernel["user"]}, actor.user)
        return Response(status_code=204)

    @app.get("/api/v1/envs")
    def environments(actor: Principal):
        return collector.environments()

    @app.get("/api/v1/history")
    def metrics(metric: str, actor: Principal, user: str | None = None,
                since: Annotated[float | None, Query(ge=0, allow_inf_nan=False)] = None,
                step: Annotated[float, Query(ge=2, allow_inf_nan=False)] = INTERVAL):
        if metric in USER_METRICS:
            user = user if user is not None else actor.user
            if not user:
                raise HTTPException(400, "A user metric requires a username")
            if not actor.can_access(user):
                raise HTTPException(403, "Access denied")
        elif metric not in SYSTEM_METRICS or user is not None:
            raise HTTPException(400, "Invalid metric or user")
        since = since if since is not None else time.time() - RETENTION
        return {"metric": metric, "user": user, "points": history.points(metric, user, since, step)}

    @app.get("/api/v1/stream")
    async def stream(actor: Principal):
        def payloads():
            system, users, kernels = collect()
            return (("system", system), ("users", users_for(users, actor)), ("kernels", scoped(kernels, actor)))

        async def events():
            while True:
                started = time.monotonic()
                for name, data in await run_in_threadpool(payloads):
                    yield f"event: {name}\ndata: {json.dumps(data)}\n\n"
                await asyncio.sleep(max(0, 2 - (time.monotonic() - started)))

        class LimitedStream(StreamingResponse):
            async def __call__(self, scope, receive, send):
                count = stream_counts.get(actor.uid, 0)
                if count >= 4:
                    response = JSONResponse({"error": "failed", "detail": "Too many streams"}, status_code=429)
                    await response(scope, receive, send)
                    return
                stream_counts[actor.uid] = count + 1
                try:
                    await super().__call__(scope, receive, send)
                finally:
                    stream_counts[actor.uid] -= 1
                    if stream_counts[actor.uid] == 0:
                        del stream_counts[actor.uid]

        return LimitedStream(events(), media_type="text/event-stream",
                                 headers={"Cache-Control": "no-cache", "X-Accel-Buffering": "no"})

    @app.post("/api/v1/users")
    def add_user(body: NewUser, actor: Administrator):
        return user_mutation("user-add", body.model_dump(), actor)

    @app.delete("/api/v1/users/{user}")
    def disable_user(user: str, actor: Administrator):
        return user_mutation("user-disable", {"user": user}, actor)

    @app.put("/api/v1/users/{user}/role")
    def set_role(user: str, body: Role, actor: Administrator):
        return user_mutation("user-role", {"user": user, "admin": body.role == "admin"}, actor)

    @app.get("/api/v1/users/{user}/keys")
    def get_keys(user: str, actor: Administrator):
        return helper.call("key-list", {"user": user}, actor.user)["keys"]

    @app.post("/api/v1/users/{user}/keys")
    def add_key(user: str, body: Key, actor: Administrator):
        return user_mutation("key-add", {"user": user, "pubkey": body.pubkey}, actor)

    @app.delete("/api/v1/users/{user}/keys/{fingerprint:path}")
    def delete_key(user: str, fingerprint: str, actor: Administrator):
        return user_mutation("key-remove", {"user": user, "fingerprint": fingerprint}, actor)

    @app.get("/api/v1/policy")
    def get_policy(actor: Administrator):
        return host.policy()

    @app.put("/api/v1/policy")
    def set_policy(body: Policy, actor: Administrator):
        return helper.call("policy-set", body.model_dump(), actor.user)

    @app.get("/api/v1/hardware")
    def hardware(actor: Principal):
        return winhost.hardware(collect()[0])

    @app.get("/api/v1/wsl")
    def wsl_status(actor: Administrator):
        system = collect()[0]
        return winhost.wsl(time.time() - system["uptime_s"])

    @app.put("/api/v1/wsl/config")
    def wsl_config(body: WslConfig, actor: Administrator):
        result = helper.call("wslconfig-set", body.model_dump(), actor.user)
        winhost.invalidate()
        return result

    @app.post("/api/v1/wsl/restart", status_code=202)
    def wsl_restart(actor: Administrator):
        return helper.call("wsl-restart", {}, actor.user)

    @app.get("/api/v1/npu/status")
    async def npu_status(actor: Principal):
        status = await gateway.status()
        if actor.role != "admin":
            if active := status["active"]:
                status["active"] = {"user": "you" if active["user"] == actor.user else "other"}
            queued = status["queued"]
            own = queued.get(actor.user, 0)
            status["queued"] = {"you": own, "others": sum(queued.values()) - own}
        return status

    @app.post("/api/v1/npu/run")
    async def npu_run(request: Request, actor: Principal):
        try:
            await gateway.sched.acquire(actor.user, {"endpoint": "run"})
        except npu.QueueFull:
            raise HTTPException(429, f"You already have {npu.MAX_QUEUED_PER_USER} NPU jobs queued")
        except npu.WaitTimeout:
            raise HTTPException(503, "NPU busy: waited too long in the queue")
        ok = False
        try:
            with BytesIO() as upload:
                async for chunk in request.stream():
                    if upload.tell() + len(chunk) > npu.MAX_BODY:
                        raise HTTPException(413, f"Body exceeds {npu.MAX_BODY} bytes")
                    upload.write(chunk)
                body = upload.getvalue()
            if not body:
                raise HTTPException(413, "Body must not be empty")
            gateway.sched.active["bytes"] = len(body)

            disconnected = False
            async with anyio.create_task_group() as tasks:
                async def watch_disconnect():
                    nonlocal disconnected
                    while (await request.receive())["type"] != "http.disconnect":
                        pass
                    disconnected = True
                    tasks.cancel_scope.cancel()

                tasks.start_soon(watch_disconnect)
                try:
                    r = await gateway.run(body)
                except anyio.get_cancelled_exc_class():
                    raise
                except (httpx.HTTPError, npu.MissingToken) as exc:
                    worker_error = exc
                else:
                    worker_error = None
                finally:
                    tasks.cancel_scope.cancel()
            if disconnected:
                raise ClientDisconnect()
            if worker_error is not None:
                raise HTTPException(502, f"NPU worker unavailable: {type(worker_error).__name__}")
            ok = r.status_code == 200
        except ClientDisconnect:
            return Response(status_code=499)
        finally:
            gateway.sched.release(ok)
        if r.status_code != 200:
            try:
                detail = r.json().get("detail") or r.json().get("error")
            except ValueError:
                detail = r.text[:300]
            raise HTTPException(400 if r.status_code < 500 else 502, f"NPU run failed: {detail}")
        return Response(r.content, media_type="application/x-npz")

    @app.get("/api/v1/audit")
    def audit(actor: Administrator, limit: Annotated[int, Query(ge=1)] = 100):
        return host.audit(limit)

    return app


app = create_app()
