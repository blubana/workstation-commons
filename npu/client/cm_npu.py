"""cm_npu: run OpenVINO models on the server's Intel NPU from any compute kernel.

    import numpy as np, openvino as ov, cm_npu
    from openvino import opset13 as ops
    a = ops.parameter([1024, 1024], np.float32, name="a")
    b = ops.parameter([1024, 1024], np.float32, name="b")
    model = ov.Model([ops.matmul(a, b, False, False)], [a, b])
    out = cm_npu.run(model, {"a": A, "b": B})        # -> dict name -> ndarray
    cm_npu.status()                                   # queue / device info

Any model OpenVINO can convert also works: cm_npu.run(ov.convert_model(torch_module,
example_input=x), {...}). The NPU needs static shapes. Jobs are queued fairly with
the other users (one NPU job at a time). Use device="CPU" or "GPU.0" (Intel iGPU) to
compare against the same pipeline.
"""
import http.client
import io
import json
import os
import socket
import tempfile

import numpy as np

SOCK = os.getenv("CM_API_SOCK", "/run/compute-mgr/api.sock")
last_timing = None


class _UnixConn(http.client.HTTPConnection):
    def __init__(self, timeout):
        super().__init__("localhost", timeout=timeout)

    def connect(self):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(self.timeout)
        self.sock.connect(SOCK)


def _request(method, path, body=None, ctype=None, timeout=900):
    c = _UnixConn(timeout)
    headers = {"Content-Type": ctype} if ctype else {}
    c.request(method, path, body=body, headers=headers)
    r = c.getresponse()
    data = r.read()
    c.close()
    if r.status != 200:
        try:
            detail = json.loads(data).get("detail")
        except ValueError:
            detail = data[:300]
        raise RuntimeError(f"NPU request failed ({r.status}): {detail}")
    return data, r.getheader("Content-Type", "")


def status():
    return json.loads(_request("GET", "/api/v1/npu/status")[0])


def _ir_bytes(model):
    import openvino as ov
    if isinstance(model, (str, os.PathLike)):
        model = ov.Core().read_model(os.fspath(model))
    with tempfile.TemporaryDirectory() as d:
        ov.save_model(model, os.path.join(d, "m.xml"), compress_to_fp16=False)
        with open(os.path.join(d, "m.xml"), "rb") as f:
            xml = f.read()
        with open(os.path.join(d, "m.bin"), "rb") as f:
            weights = f.read()
    return xml, weights


def run(model, inputs, device="NPU"):
    """model: openvino.Model or path to .xml/.onnx; inputs: {name: ndarray}. Returns {name: ndarray}."""
    global last_timing
    xml, weights = _ir_bytes(model)
    buf = io.BytesIO()
    np.savez(buf, __xml__=np.frombuffer(xml, np.uint8), __bin__=np.frombuffer(weights, np.uint8),
             __device__=np.array(device), **{k: np.ascontiguousarray(v) for k, v in inputs.items()})
    data, _ = _request("POST", "/api/v1/npu/run", buf.getvalue(), "application/x-npz")
    out = dict(np.load(io.BytesIO(data), allow_pickle=False))
    last_timing = json.loads(out.pop("__timing__").tobytes())
    return out
