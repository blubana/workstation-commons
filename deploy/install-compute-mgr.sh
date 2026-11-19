#!/bin/bash
# Run as root on the server from an unpacked repo: bash deploy/install-compute-mgr.sh
set -euo pipefail
SRC=$(cd "$(dirname "$0")/.." && pwd)
export DEBIAN_FRONTEND=noninteractive
apt-get install -y -qq python3-venv >/dev/null
id computemgr &>/dev/null || useradd --system --home /var/lib/compute-mgr --shell /usr/sbin/nologin computemgr
getent group computeadmin >/dev/null || groupadd computeadmin
install -d -m755 /opt/compute-mgr
install -d -o root -g root -m0755 /etc/compute-mgr
for file in npu.token win-info; do
  if [ -f "/etc/compute-mgr/$file" ]; then
    chown root:computemgr "/etc/compute-mgr/$file"
    chmod 0640 "/etc/compute-mgr/$file"
  fi
done
for file in win-admin site.conf known_hosts; do
  if [ -f "/etc/compute-mgr/$file" ]; then
    chown root:root "/etc/compute-mgr/$file"
    if [ "$file" = win-admin ]; then
      chmod 0600 "/etc/compute-mgr/$file"
    else
      chmod 0644 "/etc/compute-mgr/$file"
    fi
  fi
done
install -d -o root -g computemgr -m0750 /var/log/compute-mgr
audit=/var/log/compute-mgr/audit.jsonl
if [ ! -e "$audit" ]; then
  install -o root -g computemgr -m0640 /dev/null "$audit"
fi
chown root:computemgr "$audit"
chmod 0640 "$audit"
install -o root -g root -m755 "$SRC/helper/compute-mgr-helper" /usr/local/libexec/compute-mgr-helper
# Seed the helper's managed-account registry once: current compute members without OS admin groups.
install -d -m700 -o root -g root /var/lib/compute-mgr-helper
if [ ! -f /var/lib/compute-mgr-helper/managed.json ]; then
  python3 -I -c 'import grp,json,pwd; priv={"sudo","adm","wheel","root","admin"}; m=[u for u in grp.getgrnam("compute").gr_mem if not ({g.gr_name for g in grp.getgrall() if u in g.gr_mem} & priv) and pwd.getpwnam(u).pw_uid>=1000]; print(json.dumps(sorted(m)))' > /var/lib/compute-mgr-helper/managed.json
  chmod 600 /var/lib/compute-mgr-helper/managed.json
fi
install -o root -g root -m440 "$SRC/deploy/sudoers" /etc/sudoers.d/compute-mgr; visudo -cf /etc/sudoers.d/compute-mgr >/dev/null
rm -rf /opt/compute-mgr/backend /opt/compute-mgr/web /etc/pam.d/compute-mgr /etc/compute-mgr/secret /usr/local/bin/compute-mgr-token
gpasswd -d computemgr shadow 2>/dev/null || true
cp -r "$SRC/backend" /opt/compute-mgr/backend
python3 -m venv /opt/compute-mgr/backend/.venv
(
  cd /opt/compute-mgr/backend
  .venv/bin/pip install --require-hashes -r requirements.lock
  .venv/bin/pip install --no-deps .
)
[ -x "$SRC/tui/cm-top" ] && install -m755 "$SRC/tui/cm-top" /usr/local/bin/cm-top
# NPU client library for every shared env
install -d -m755 /opt/compute/lib && install -m644 "$SRC/npu/client/cm_npu.py" /opt/compute/lib/cm_npu.py
for e in /opt/conda/envs/*; do sp=$(ls -d "$e"/lib/python3*/site-packages | head -1); echo /opt/compute/lib > "$sp/zz_compute_lib.pth"; done
install -m644 "$SRC/backend/compute-mgrd.service" /etc/systemd/system/compute-mgrd.service
mkdir -p /etc/systemd/system/compute-mgrd.service.d
printf '[Service]\nEnvironment=PATH=/usr/lib/wsl/lib:/usr/local/bin:/usr/bin:/bin\n' > /etc/systemd/system/compute-mgrd.service.d/env.conf
systemctl daemon-reload; systemctl enable -q compute-mgrd; systemctl restart compute-mgrd
echo installed
