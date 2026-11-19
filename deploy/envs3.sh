set -e
for e in sci sci-spyder5 sci-legacy; do
  P=/opt/conda/envs/$e; SP=$($P/bin/python -c 'import site;print(site.getsitepackages()[0])')
  echo "import os; os.environ.setdefault('CUDA_PATH', '$P/targets/x86_64-linux')" > $SP/zz_cuda_path.pth
  $P/bin/python -c "import cupy,numpy,sys;print('$e',sys.version.split()[0],numpy.__version__,cupy.cuda.runtime.getDeviceProperties(0)['name'].decode(), float(cupy.arange(10).sum()))"
done
