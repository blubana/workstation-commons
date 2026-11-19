import json
import os
import subprocess


class HelperError(Exception):
    def __init__(self, code, detail, status):
        self.code, self.detail, self.status = code, detail, status


def call(op, args, actor):
    path = os.getenv("CM_HELPER", "/usr/local/libexec/compute-mgr-helper")
    try:
        result = subprocess.run(
            ["sudo", "-n", path, op, json.dumps({**args, "actor": actor})],
            capture_output=True, text=True, timeout=30,
        )
        body = json.loads(result.stdout)
    except (OSError, ValueError, subprocess.TimeoutExpired) as exc:
        raise HelperError("failed", "Helper unavailable or returned an invalid response", 502) from exc
    if not isinstance(body, dict):
        raise HelperError("failed", "Helper returned an invalid response", 502)
    if result.returncode or body.get("ok") is not True:
        status = {"validation": 400, "forbidden": 403, "failed": 500}
        code = body.get("error")
        if not isinstance(code, str) or code not in status:
            raise HelperError("failed", "Helper operation failed", 502)
        raise HelperError(code, body.get("detail", "Helper operation failed"), status[code])
    return body
