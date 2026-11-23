"""compute-mgr: default per-process GPU memory cap (75% of VRAM) so the local operator keeps headroom.

Loaded from zzz_cm_gpu.pth in every shared env. Override: CM_GPU_FRACTION=1 (or 0.5 …) before Python starts.
torch: applied lazily at CUDA init via torch.cuda._lazy_call; cupy: CUPY_GPU_MEMORY_LIMIT default.
Never raises: any failure leaves the process unlimited.
"""
import os, sys

try:
    FRACTION = min(1.0, max(0.05, float(os.environ.get("CM_GPU_FRACTION", "0.75"))))
except ValueError:
    FRACTION = 0.75

if FRACTION < 1.0:
    os.environ.setdefault("CUPY_GPU_MEMORY_LIMIT", f"{int(FRACTION * 100)}%")

    def _apply_torch(cuda=None):
        try:
            if cuda is None:
                import torch.cuda as cuda
            def _cap():
                for i in range(cuda.device_count()):
                    cuda.set_per_process_memory_fraction(FRACTION, i)
            cuda._lazy_call(_cap)
        except Exception:
            pass

    class _TorchHook:
        def find_spec(self, name, path=None, target=None):
            if name != "torch.cuda":
                return None
            sys.meta_path.remove(self)
            import importlib.util
            spec = importlib.util.find_spec(name)
            if spec is None or spec.loader is None:
                return spec
            orig = spec.loader.exec_module
            def exec_module(module):
                orig(module)
                _apply_torch(module)   # torch.cuda is not yet an attribute of torch here
            spec.loader.exec_module = exec_module
            return spec

    if "torch.cuda" in sys.modules:
        _apply_torch()
    else:
        sys.meta_path.insert(0, _TorchHook())
