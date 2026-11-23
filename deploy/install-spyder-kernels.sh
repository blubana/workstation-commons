#!/bin/bash
# Per-Spyder-version kernel venvs: /opt/compute/kernels/sk-<minor>, on top of sci-legacy (py3.11 stack).
# Each Spyder minor release accepts only one spyder-kernels minor; see `spyder-kernel --list`.
set -euo pipefail
BASE=/opt/conda/envs/sci-legacy/bin/python
DEST=${DEST:-/opt/compute/kernels}
LOCK="$(cd "$(dirname "$0")" && pwd)/spyder-kernels-lock"   # hash-locked venv-local deps (regenerate: see README there)
VERSIONS="0.5.2 1.9.4 1.10.3 2.0.5 2.1.3 2.2.1 2.3.3 2.4.4 3.0.5"
install -d -m755 "$DEST"
for v in $VERSIONS; do
  d="$DEST/sk-${v%.*}"
  [ -x "$d/bin/python" ] && "$d/bin/python" -c "import spyder_kernels as s,sys; sys.exit(s.__version__!='$v')" 2>/dev/null && continue
  rm -rf "$d"; "$BASE" -m venv --system-site-packages "$d"
  "$d/bin/pip" install -q --disable-pip-version-check --only-binary :all: --no-cache-dir \
      --require-hashes --no-deps -r "$LOCK/sk-${v%.*}.txt"
  "$d/bin/python" -c 'import spyder_kernels.console' || { echo "sk-${v%.*}: import failed"; exit 1; }
  echo "sk-${v%.*}: $("$d/bin/python" -c 'import spyder_kernels;print(spyder_kernels.__version__)')"
done
chmod -R go-w "$DEST"
if [ "$DEST" = /opt/compute/kernels ]; then install -m755 "$(dirname "$0")/spyder-kernel" /usr/local/bin/spyder-kernel; fi
