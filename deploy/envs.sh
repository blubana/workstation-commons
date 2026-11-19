set -euo pipefail
echo 'export PATH=/usr/lib/wsl/lib:$PATH' > /etc/profile.d/wsl-gpu.sh
grep -q wsl-gpu /etc/bash.bashrc || echo '. /etc/profile.d/wsl-gpu.sh; . /etc/profile.d/conda.sh' >> /etc/bash.bashrc
C=/opt/conda/bin/conda
# Spyder 6.x
$C create -y -q -p /opt/conda/envs/sci -c conda-forge python=3.12 numpy scipy pandas matplotlib numba cupy cuda-version=12.8 'spyder-kernels>=3.1,<3.2' ipykernel >/dev/null
# Spyder 5.x
$C create -y -q -p /opt/conda/envs/sci-spyder5 -c conda-forge python=3.11 numpy scipy pandas matplotlib numba cupy cuda-version=12.8 'spyder-kernels>=2.5,<2.6' >/dev/null
# Spyder 3.2.x: plain ipykernel (3.2 predates spyder-kernels)
$C create -y -q -p /opt/conda/envs/sci-legacy -c conda-forge python=3.11 numpy scipy pandas matplotlib numba cupy cuda-version=12.8 'ipykernel>=6,<7' >/dev/null
chmod -R u+rwX,go+rX,go-w /opt/conda
cat > /usr/local/bin/spyder-kernel <<'K'
#!/bin/bash
# usage: spyder-kernel [sci|sci-spyder5|sci-legacy]   (default sci)
ENV=${1:-sci}; PY=/opt/conda/envs/$ENV/bin/python
[ -x "$PY" ] || { echo "unknown env $ENV"; exit 1; }
RT=$HOME/.local/share/jupyter/runtime; mkdir -p "$RT"
CF=$RT/kernel-$ENV-$$.json
if [ -x /opt/conda/envs/$ENV/bin/python ] && $PY -c 'import spyder_kernels' 2>/dev/null; then
  MOD=spyder_kernels.console; else MOD=ipykernel_launcher; fi
nohup $PY -m $MOD --ip=127.0.0.1 -f "$CF" >"$RT/kernel-$ENV-$$.log" 2>&1 &
for i in $(seq 20); do [ -s "$CF" ] && break; sleep 0.5; done
echo "connection file: $CF"; cat "$CF"
K
chmod 755 /usr/local/bin/spyder-kernel
for e in sci sci-spyder5 sci-legacy; do /opt/conda/envs/$e/bin/python -c "import cupy,numpy;print('$e',numpy.__version__,cupy.cuda.runtime.getDeviceCount(), float(cupy.arange(10).sum()))"; done
