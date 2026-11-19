#!/bin/bash
# Install the shared data-science / ML stack into the shared envs. Run as root on the server.
set -uo pipefail
C=/opt/conda/bin/conda
CONDA_PKGS="numpy scipy pandas polars pyarrow matplotlib seaborn plotly bokeh altair
 scikit-learn scikit-image statsmodels sympy networkx numba cupy xarray netcdf4 h5py zarr
 dask distributed joblib tqdm rich ipywidgets openpyxl xlrd sqlalchemy duckdb
 xgboost lightgbm catboost optuna shap imbalanced-learn umap-learn hdbscan
 opencv pillow tifffile pywavelets nltk spacy gensim
 requests beautifulsoup4 lxml pydantic"
PIP_TORCH="torch torchvision torchaudio --index-url https://download.pytorch.org/whl/cu128"
PIP_PKGS="transformers datasets accelerate sentence-transformers tokenizers safetensors evaluate peft
 onnx onnxruntime-gpu openvino ovmsclient tritonclient[http] lightning torchmetrics timm einops
 tensorboard mlflow-skinny"
for e in sci sci-spyder5 sci-legacy; do
  P=/opt/conda/envs/$e
  echo "=== $e conda"; $C install -y -q -p $P -c conda-forge $CONDA_PKGS cuda-version=12.8 >/dev/null 2>&1 || echo "WARN conda $e"
  echo "=== $e torch"; $P/bin/pip install -q $PIP_TORCH 2>&1 | tail -1
  echo "=== $e pip";   $P/bin/pip install -q $PIP_PKGS 2>&1 | tail -1
  $P/bin/python - <<'PY'
import importlib, sys
mods = "numpy scipy pandas polars sklearn xgboost lightgbm catboost statsmodels cupy torch torchvision transformers onnxruntime openvino cv2 matplotlib seaborn plotly dask".split()
bad = [m for m in mods if importlib.util.find_spec(m) is None]
import torch
print(sys.prefix, "torch", torch.__version__, "cuda", torch.cuda.is_available(), torch.cuda.get_device_name(0) if torch.cuda.is_available() else "-", "missing:", bad)
PY
done
chmod -R a+rX /opt/conda
