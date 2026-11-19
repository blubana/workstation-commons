"""cm-npu worker: runs OpenVINO models on the Intel NPU of the Windows host.

Runs on Windows (scheduled task at boot), listens on 127.0.0.1 only. Requests come
exclusively from the compute-mgr gateway in WSL (mirrored networking => same loopback),
authenticated with a shared token. The gateway serialises requests, so this server
handles one request at a time.

POST /run     body: .npz with "__xml__" (uint8), "__bin__" (uint8), optional "__device__",
              and the input tensors by name  ->  .npz of outputs (+ "__timing__" json bytes)
GET  /status  -> JSON: devices, NPU name/driver, cache
"""
import hashlib
import hmac
import io
import json
import os
import sys
import time
from collections import OrderedDict
from http.server import BaseHTTPRequestHandler, HTTPServer

import numpy as np
import openvino as ov

HOST = "127.0.0.1"
PORT = int(os.getenv("CM_NPU_PORT", "8102"))
TOKEN_FILE = os.getenv("CM_NPU_TOKEN_FILE", r"C:\cm-npu\token")
CACHE_DIR = os.getenv("CM_NPU_CACHE", r"C:\cm-npu\cache")
MAX_BODY = 1 << 30           # 1 GiB
MAX_MODELS = 8
ALLOWED_DEVICES = {"NPU", "CPU", "GPU.0"}   # GPU.0 = Intel iGPU (the RTX is used from WSL)

core = ov.Core()
os.makedirs(CACHE_DIR, exist_ok=True)
core.set_property({"CACHE_DIR": CACHE_DIR})
compiled = OrderedDict()     # (sha, device) -> CompiledModel
stats = {"runs": 0, "errors": 0, "compiles": 0, "started": time.time()}
with open(TOKEN_FILE) as f:
    TOKEN = f.read().strip()


def device_info():
    out = {}
    for d in core.available_devices:
        info = {"name": core.get_property(d, "FULL_DEVICE_NAME")}
        if d == "NPU":
            for prop in ("NPU_DRIVER_VERSION", "DEVICE_ARCHITECTURE", "NPU_DEVICE_TOTAL_MEM_SIZE"):
                try:
                    info[prop.lower()] = str(core.get_property(d, prop))
                except Exception:
                    pass
        out[d] = info
    return out


def get_compiled(xml, weights, device):
    sha = hashlib.sha256(xml + b"\0" + weights).hexdigest()
    key = (sha, device)
    if key in compiled:
        compiled.move_to_end(key)
        return compiled[key], False, sha
    model = core.read_model(xml.decode("utf-8"), ov.Tensor(np.frombuffer(weights, dtype=np.uint8)))
    cm = core.compile_model(model, device)
    compiled[key] = cm
    stats["compiles"] += 1
    while len(compiled) > MAX_MODELS:
        compiled.popitem(last=False)
    return cm, True, sha


class Handler(BaseHTTPRequestHandler):
    server_version = "cm-npu/1"

    def log_message(self, fmt, *args):
        sys.stderr.write("%s %s\n" % (time.strftime("%Y-%m-%dT%H:%M:%S"), fmt % args))

    def _auth(self):
        given = self.headers.get("Authorization", "").removeprefix("Bearer ")
        if not hmac.compare_digest(given.encode(), TOKEN.encode()):
            self._json(401, {"error": "unauthorized"})
            return False
        return True

    def _json(self, code, obj):
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if not self._auth():
            return
        if self.path != "/status":
            return self._json(404, {"error": "not_found"})
        self._json(200, {"devices": device_info(), "cached_models": len(compiled), **stats,
                         "openvino": ov.get_version()})

    def do_POST(self):
        if not self._auth():
            return
        if self.path != "/run":
            return self._json(404, {"error": "not_found"})
        n = int(self.headers.get("Content-Length", "0"))
        if n <= 0 or n > MAX_BODY:
            return self._json(413, {"error": "body_size", "detail": f"0 < size <= {MAX_BODY}"})
        t0 = time.perf_counter()
        try:
            data = np.load(io.BytesIO(self.rfile.read(n)), allow_pickle=False)
            xml, weights = data["__xml__"].tobytes(), data["__bin__"].tobytes()
            device = str(data["__device__"]) if "__device__" in data.files else "NPU"
            if device not in ALLOWED_DEVICES:
                return self._json(400, {"error": "validation", "detail": f"device must be one of {sorted(ALLOWED_DEVICES)}"})
            cm, fresh, sha = get_compiled(xml, weights, device)
            t1 = time.perf_counter()
            inputs = {k: data[k] for k in data.files if not k.startswith("__")}
            req = cm.create_infer_request()
            res = req.infer(inputs)
            t2 = time.perf_counter()
            outs = {}
            for port, value in res.items():
                name = port.get_any_name() if port.get_names() else f"out{len(outs)}"
                outs[name] = np.asarray(value)
            timing = {"compile_s": round(t1 - t0, 4) if fresh else 0.0, "infer_s": round(t2 - t1, 4),
                      "cached": not fresh, "model_sha256": sha, "device": device}
            outs["__timing__"] = np.frombuffer(json.dumps(timing).encode(), dtype=np.uint8)
            buf = io.BytesIO()
            np.savez(buf, **outs)
            body = buf.getvalue()
            stats["runs"] += 1
        except Exception as exc:   # report model/inputs errors to the caller
            stats["errors"] += 1
            return self._json(400, {"error": "run_failed", "detail": f"{exc.__class__.__name__}: {str(exc)[:500]}"})
        self.send_response(200)
        self.send_header("Content-Type", "application/x-npz")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


if __name__ == "__main__":
    log_dir = os.getenv("CM_NPU_LOGS", r"C:\cm-npu\logs")
    os.makedirs(log_dir, exist_ok=True)
    sys.stdout = sys.stderr = open(os.path.join(log_dir, "worker.log"), "a", buffering=1)
    print("cm-npu worker devices:", core.available_devices, flush=True)
    HTTPServer((HOST, PORT), Handler).serve_forever()
