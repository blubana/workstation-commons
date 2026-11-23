import json

from fastapi.testclient import TestClient

from compute_mgr import helper, host
from compute_mgr.app import create_app
from compute_mgr.history import History
from compute_mgr.identity import Identity


def test_scoping(collector, tmp_path, monkeypatch):
    actions = []
    monkeypatch.setattr(helper, "call", lambda *args: actions.append(args) or {"ok": True, "keys": []})
    app = create_app(collector=collector, history=History(tmp_path / "db"),
                     resolve_uid=lambda uid: Identity("alice", uid, "user"), sample=False)

    async def unix(scope, receive, send):
        scope["compute_mgr.peer_uid"] = 1001
        await app(scope, receive, send)

    with TestClient(unix) as client:
        assert [item["user"] for item in client.get("/api/v1/users").json()] == ["alice"]
        assert [item["user"] for item in client.get("/api/v1/kernels").json()] == ["alice"]
        assert client.delete("/api/v1/kernels/bob-24").status_code == 404
        assert client.get("/api/v1/history?metric=user_mem&user=bob").status_code == 403
        for metric in ("net_rx", "net_tx", "disk_read", "disk_write", "gpu_temp"):
            response = client.get("/api/v1/history", params={"metric": metric})
            assert response.status_code == 200
            assert response.json() == {"metric": metric, "user": None, "points": []}
        assert client.get("/api/v1/audit").status_code == 403
        assert client.post("/api/v1/users", json={"user": "bob", "pubkey": "key"}).status_code == 403
        assert client.delete("/api/v1/kernels/alice-23").status_code == 204
        assert ("kernel-stop", {"pid": 23, "owner": "alice"}, "alice") in actions

    # without peer credentials (no Unix socket) every endpoint is refused
    with TestClient(app) as client:
        assert client.get("/api/v1/auth/me", headers={"X-UID": "1001"}).status_code == 401
        assert client.get("/api/v1/users").status_code == 401
        assert client.post("/api/v1/auth/login", json={"username": "a", "password": "b"}).status_code in (404, 405)
        assert client.get("/").status_code == 404


def test_admin_helper_routes(collector, tmp_path, monkeypatch):
    actions = []

    def call(op, args, actor):
        actions.append((op, args, actor))
        return {"ok": True, "keys": []}

    monkeypatch.setattr(helper, "call", call)
    app = create_app(collector=collector, history=History(tmp_path / "db"),
                     resolve_uid=lambda uid: Identity("admin", uid, "admin"), sample=False)

    async def unix(scope, receive, send):
        scope["compute_mgr.peer_uid"] = 1000
        await app(scope, receive, send)

    policy = {"mem_low_gb": 16, "mem_high_gb": 80, "total_max_gb": 104, "cpu_weight": 100}
    with TestClient(unix) as client:
        assert len(client.get("/api/v1/users").json()) == 2
        assert client.get("/api/v1/users/alice/keys").json() == []
        assert client.delete("/api/v1/users/alice/keys/SHA256:abc%2Fdef").status_code == 200
        assert client.put("/api/v1/policy", json=policy).status_code == 200
        assert client.delete("/api/v1/kernels/bob-24").status_code == 204
    assert ("key-remove", {"user": "alice", "fingerprint": "SHA256:abc/def"}, "admin") in actions
    assert ("policy-set", policy, "admin") in actions
    assert ("kernel-stop", {"pid": 24, "owner": "bob"}, "admin") in actions


def test_audit_reads_only_needed_tail(tmp_path, monkeypatch):
    path = tmp_path / "audit.jsonl"
    event = {"time": 1, "actor": "admin", "action": "test", "target": "x", "ok": True}
    old = json.dumps(dict(event, time=0)).encode() + b"\n"
    events = [dict(event, target="界" * 23000), dict(event, time=2)]
    path.write_bytes(
        old * 10000 + json.dumps(events[0], ensure_ascii=False).encode() + b"\r\n"
        + b'{"phase":"attempt"}\n' + b"x" * 100000 + b"\nnull\n"
        + json.dumps(events[1]).encode()
    )

    class TailOnly:
        def open(self, mode):
            log = path.open(mode)
            original_read = log.read

            def read(size=-1):
                assert log.tell() > len(old) * 9000
                return original_read(size)

            log.read = read
            return log

    monkeypatch.setattr(host, "AUDIT_LOG", TailOnly())
    assert host.audit(2) == events[::-1]

    monkeypatch.setattr(host, "AUDIT_LOG", path)
    path.write_bytes(json.dumps(event).encode())
    assert host.audit(100) == [event]
    path.unlink()
    assert host.audit(100) == []
