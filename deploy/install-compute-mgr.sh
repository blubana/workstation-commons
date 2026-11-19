#!/bin/bash
# Run as root on the server from an unpacked repo: bash deploy/install-compute-mgr.sh
set -euo pipefail
SRC=$(cd "$(dirname "$0")/.." && pwd)
export DEBIAN_FRONTEND=noninteractive
apt-get install -y -qq python3-venv >/dev/null
id computemgr &>/dev/null || useradd --system --home /var/lib/compute-mgr --shell /usr/sbin/nologin computemgr
getent group computeadmin >/dev/null || groupadd computeadmin
install -d -m755 /opt/compute-mgr /etc/compute-mgr
install -o root -g root -m755 "$SRC/helper/compute-mgr-helper" /usr/local/libexec/compute-mgr-helper
install -o root -g root -m440 "$SRC/deploy/sudoers" /etc/sudoers.d/compute-mgr; visudo -cf /etc/sudoers.d/compute-mgr >/dev/null
rm -rf /opt/compute-mgr/backend /opt/compute-mgr/web /etc/pam.d/compute-mgr /etc/compute-mgr/secret /usr/local/bin/compute-mgr-token
gpasswd -d computemgr shadow 2>/dev/null || true
cp -r "$SRC/backend" /opt/compute-mgr/backend
python3 -m venv /opt/compute-mgr/backend/.venv
/opt/compute-mgr/backend/.venv/bin/pip install -q /opt/compute-mgr/backend
[ -x "$SRC/tui/cm-top" ] && install -m755 "$SRC/tui/cm-top" /usr/local/bin/cm-top
# NPU client library for every shared env
install -d -m755 /opt/compute/lib && install -m644 "$SRC/npu/client/cm_npu.py" /opt/compute/lib/cm_npu.py
for e in /opt/conda/envs/*; do sp=$(ls -d "$e"/lib/python3*/site-packages | head -1); echo /opt/compute/lib > "$sp/zz_compute_lib.pth"; done
install -m644 "$SRC/backend/compute-mgrd.service" /etc/systemd/system/compute-mgrd.service
mkdir -p /etc/systemd/system/compute-mgrd.service.d
printf '[Service]\nEnvironment=PATH=/usr/lib/wsl/lib:/usr/local/bin:/usr/bin:/bin\n' > /etc/systemd/system/compute-mgrd.service.d/env.conf
systemctl daemon-reload; systemctl enable -q compute-mgrd; systemctl restart compute-mgrd
echo installed
