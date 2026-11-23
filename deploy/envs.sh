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
install -m 755 "$(dirname "$0")/spyder-kernel" /usr/local/bin/spyder-kernel   # single launcher (also used by install-spyder-kernels.sh)
install -m 644 "$(dirname "$0")/cm-threads.sh" /etc/profile.d/cm-threads.sh
chmod 755 /usr/local/bin/spyder-kernel
for e in sci sci-spyder5 sci-legacy; do /opt/conda/envs/$e/bin/python -c "import cupy,numpy;print('$e',numpy.__version__,cupy.cuda.runtime.getDeviceCount(), float(cupy.arange(10).sum()))"; done
