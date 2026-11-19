import asyncio
import json
import subprocess
import time

import pytest
from fastapi.testclient import TestClient

from compute_mgr import helper, npu
from compute_mgr.app import create_app
from compute_mgr.history import History
from compute_mgr.identity import Identity


@pytest.fixture
def api(collector, tmp_path):
    history = History(tmp_path / "db")
    app = create_app(collector=collector, history=history, sample=False,
                     resolve_uid=lambda uid: Identity("alice", uid, "admin" if uid == 1000 else "user"))

    async def unix(scope, receive, send):
        if scope["type"] == "http":
            scope.setdefault("compute_mgr.peer_uid", 1001)
        await app(scope, receive, send)

    return unix, history


def request_scope(path, uid=1001, method="GET", headers=()):
    return {"type": "http", "asgi": {"version": "3.0"}, "http_version": "1.1",
            "method": method, "scheme": "http", "path": path, "headers": list(headers),
            "query_string": b"", "compute_mgr.peer_uid": uid}


@pytest.mark.parametrize("headers", [[], [(b"content-length", b"1")]])
def test_npu_body_cap(api, monkeypatch, headers):
    app, _ = api
    monkeypatch.setattr(npu, "MAX_BODY", 8)
    sched = None
    acquire = npu.Scheduler.acquire

    async def capture(self, *args):
        nonlocal sched
        sched = self
        await acquire(self, *args)

    monkeypatch.setattr(npu.Scheduler, "acquire", capture)
    chunks = iter([b"12345", b"67890"])
    sent = []

    async def receive():
        assert sched.active["user"] == "alice"
        return {"type": "http.request", "body": next(chunks), "more_body": True}

    async def send(message):
        sent.append(message)

    asyncio.run(app(request_scope("/api/v1/npu/run", method="POST", headers=headers), receive, send))
    assert sent[0]["status"] == 413
    assert sched.active is None


def test_npu_status_roles(api, monkeypatch):
    app, _ = api
    active = {"user": "alice", "bytes": 52, "started": 1, "endpoint": "run"}
    queued = {"alice": 2, "bob": 3, "carol": 1}

    async def status(self):
        return {"active": active, "queued": queued}

    monkeypatch.setattr(npu.Gateway, "status", status)
    with TestClient(app) as client:
        assert client.get("/api/v1/npu/status").json() == {
            "active": {"user": "you"}, "queued": {"you": 2, "others": 4},
        }
        active["user"] = "bob"
        assert client.get("/api/v1/npu/status").json()["active"] == {"user": "other"}

    async def admin(scope, receive, send):
        scope["compute_mgr.peer_uid"] = 1000
        await app(scope, receive, send)

    with TestClient(admin) as client:
        assert client.get("/api/v1/npu/status").json() == {"active": active, "queued": queued}


def test_history_bucket_limit(api):
    app, history = api
    now = time.time()
    since = now - 9000
    with TestClient(app) as client:
        for i in range(3000):
            history.record(since + i * 3, [("cpu_percent", "", i)])
        params = {"metric": "cpu_percent", "since": since, "step": 2}
        response = client.get("/api/v1/history", params=params)
        assert response.status_code == 200
        points = response.json()["points"]
        assert 1000 < len(points) <= 2000
        assert points[-1][1] >= 2998
        assert client.get("/api/v1/history", params={**params, "step": 1.9}).status_code == 422


def test_stream_limit_is_per_uid_and_released_on_disconnect(api, monkeypatch):
    app, _ = api
    monkeypatch.setattr(helper, "call", lambda *args: {"ok": True, "keys": []})

    async def main():
        async def open_stream(uid):
            started = asyncio.get_running_loop().create_future()
            disconnect = asyncio.Event()

            async def receive():
                await disconnect.wait()
                return {"type": "http.disconnect"}

            async def send(message):
                if message["type"] == "http.response.start":
                    started.set_result(message["status"])

            task = asyncio.create_task(app(request_scope("/api/v1/stream", uid), receive, send))
            return await asyncio.wait_for(started, 2), disconnect, task

        streams = [await open_stream(1001) for _ in range(5)]
        assert [status for status, _, _ in streams] == [200, 200, 200, 200, 429]
        streams.append(await open_stream(1000))
        assert streams[-1][0] == 200
        streams[0][1].set()
        await asyncio.wait_for(streams[0][2], 2)
        streams.append(await open_stream(1001))
        assert streams[-1][0] == 200
        for _, disconnect, _ in streams:
            disconnect.set()
        await asyncio.wait_for(asyncio.gather(*(task for _, _, task in streams)), 2)

    asyncio.run(main())


def test_disable_reports_helper_failure(api, monkeypatch):
    app, _ = api
    detail = "disable incomplete: processes still running"

    def fail(*args, **kwargs):
        return subprocess.CompletedProcess([], 3, json.dumps({"error": "failed", "detail": detail}))

    monkeypatch.setattr(helper.subprocess, "run", fail)

    async def admin(scope, receive, send):
        scope["compute_mgr.peer_uid"] = 1000
        await app(scope, receive, send)

    with TestClient(admin) as client:
        response = client.delete("/api/v1/users/bob")
    assert response.status_code == 500
    assert response.json() == {"error": "failed", "detail": detail}


def test_npu_disconnect_releases_running_slot(api, monkeypatch):
    app, _ = api
    started = asyncio.Event()
    sched = None
    acquire = npu.Scheduler.acquire

    async def capture(self, *args):
        nonlocal sched
        sched = self
        await acquire(self, *args)

    async def run_worker(self, body):
        started.set()
        await asyncio.Future()

    monkeypatch.setattr(npu.Scheduler, "acquire", capture)
    monkeypatch.setattr(npu.Gateway, "run", run_worker)
    sent = []
    request_sent = False

    async def receive():
        nonlocal request_sent
        if request_sent:
            await started.wait()
            return {"type": "http.disconnect"}
        request_sent = True
        return {"type": "http.request", "body": b"npz"}

    async def send(message):
        sent.append(message)

    async def main():
        await asyncio.wait_for(app(request_scope("/api/v1/npu/run", method="POST"), receive, send), 2)

    asyncio.run(main())
    assert sent[0]["status"] == 499
    assert sched.active is None


def test_npu_run_success_returns_worker_output(api, monkeypatch):
    """Regression: a normally completed run must return 200, not 499 (false disconnect)."""
    app, _ = api

    class Reply:
        status_code = 200
        content = b"NPZDATA"

    async def run_worker(self, body):
        await asyncio.sleep(0.05)
        assert body == b"payload"
        return Reply()

    monkeypatch.setattr(npu.Gateway, "run", run_worker)
    messages = iter([{"type": "http.request", "body": b"payload", "more_body": False}])
    sent = []

    async def receive():
        try:
            return next(messages)
        except StopIteration:
            await asyncio.sleep(3600)          # client stays connected

    async def send(message):
        sent.append(message)

    asyncio.run(app(request_scope("/api/v1/npu/run", method="POST"), receive, send))
    assert sent[0]["status"] == 200
    assert b"".join(m.get("body", b"") for m in sent[1:]) == b"NPZDATA"


@pytest.mark.parametrize("created", [True, False])
def test_new_account_purges_reused_name_history(api, monkeypatch, created):
    """A1: only a newly created account (name reuse) loses old samples; re-enable keeps them."""
    app, history = api
    history.initialize()
    now = time.time()
    history.record(now - 60, [("user_mem", "bob", 1.0), ("user_mem", "carol", 2.0)])
    monkeypatch.setattr(helper, "call", lambda op, args, actor: {"ok": True, "user": "bob", "created": created})
    sent = []

    async def receive():
        return {"type": "http.request", "body": json.dumps({"user": "bob", "pubkey": "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA"}).encode(), "more_body": False}

    async def send(m):
        sent.append(m)

    asyncio.run(app(request_scope("/api/v1/users", uid=1000, method="POST",
                                  headers=[(b"content-type", b"application/json")]), receive, send))
    assert sent[0]["status"] == 200, sent
    left = {u for u in ("bob", "carol") if history.points("user_mem", u, now - 3600, 10)}
    assert ("bob" in left) is (not created)
    assert "carol" in left
