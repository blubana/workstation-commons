#!/bin/bash
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
mkdir -p /etc/ssh/sshd_config.d
printf 'Port 2200\n' > /etc/ssh/sshd_config.d/10-compute.conf
dpkg --configure -a || true
apt-get install -y -qq openssh-server btop nvtop htop tmux git curl acl quota >/dev/null
# admin user
id admin &>/dev/null || useradd -m -s /bin/bash -G sudo admin
install -d -m700 -o admin -g admin /home/admin/.ssh
install -m600 -o admin -g admin "${ADMIN_PUBKEY_FILE:?set ADMIN_PUBKEY_FILE}" /home/admin/.ssh/authorized_keys
echo 'admin ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/admin; chmod 440 /etc/sudoers.d/admin
groupadd -f compute
# sshd on 2200, keys only
cat > /etc/ssh/sshd_config.d/10-compute.conf <<'C'
Port 2200
PasswordAuthentication no
KbdInteractiveAuthentication no
PermitRootLogin no
AllowGroups compute sudo
AllowTcpForwarding yes
ClientAliveInterval 60
C
systemctl disable --now ssh.socket 2>/dev/null || true
systemctl enable ssh; systemctl restart ssh
# per-user limits
mkdir -p /etc/systemd/system/user-.slice.d
cat > /etc/systemd/system/user-.slice.d/50-limits.conf <<'C'
[Slice]
MemoryMax=28G
MemoryHigh=24G
CPUQuota=1600%
TasksMax=4096
C
systemctl daemon-reload
# shared Miniforge
if [ ! -x /opt/conda/bin/conda ]; then
  curl -fsSL -o /tmp/mf.sh https://github.com/conda-forge/miniforge/releases/latest/download/Miniforge3-Linux-x86_64.sh
  bash /tmp/mf.sh -b -p /opt/conda
fi
cat > /etc/profile.d/conda.sh <<'C'
. /opt/conda/etc/profile.d/conda.sh
C
/opt/conda/bin/conda config --system --set channel_priority strict
/opt/conda/bin/conda config --system --add envs_dirs '~/.conda/envs'
/opt/conda/bin/pip install -q nvitop
ln -sf /opt/conda/bin/nvitop /usr/local/bin/nvitop
echo ok
