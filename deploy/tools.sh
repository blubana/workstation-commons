set -e
cat > /usr/local/sbin/compute-adduser <<'A'
#!/bin/bash
# usage: compute-adduser <name> "<ssh-pubkey>"
set -e; U=$1; K=$2; [ -n "$U" ] && [ -n "$K" ] || { echo "usage: $0 name 'ssh-ed25519 ...'"; exit 1; }
id "$U" &>/dev/null || useradd -m -s /bin/bash -G compute "$U"
install -d -m700 -o "$U" -g "$U" /home/$U/.ssh /home/$U/data
echo "$K" >> /home/$U/.ssh/authorized_keys; chown "$U:$U" /home/$U/.ssh/authorized_keys; chmod 600 /home/$U/.ssh/authorized_keys
loginctl enable-linger "$U"   # kernels survive logout
echo "added $U"
A
chmod 750 /usr/local/sbin/compute-adduser
# per-user summary panel
cat > /usr/local/bin/compute-users <<'S'
#!/bin/bash
while true; do clear; date '+%F %T'; printf '%-12s %8s %6s %8s %s\n' USER MEM_GB CPU% KERNELS GPU_MiB
for u in $(getent group compute | cut -d: -f4 | tr , ' '); do
  uid=$(id -u $u); m=$(cat /sys/fs/cgroup/user.slice/user-$uid.slice/memory.current 2>/dev/null || echo 0)
  c=$(ps -u $u -o %cpu= 2>/dev/null | awk '{s+=$1}END{printf "%.0f",s}')
  k=$(pgrep -u $u -fc 'ipykernel|spyder_kernels' || true)
  g=$(nvidia-smi --query-compute-apps=pid,used_memory --format=csv,noheader,nounits 2>/dev/null | while IFS=', ' read p mem; do [ "$(ps -o user= -p $p 2>/dev/null)" = "$u" ] && echo $mem; done | awk '{s+=$1}END{print s+0}')
  printf '%-12s %8.1f %6s %8s %s\n' $u $(echo "$m/1073741824" | bc -l) "${c:-0}" "$k" "$g"
done; sleep 3; done
S
chmod 755 /usr/local/bin/compute-users
cat > /usr/local/bin/compute-dash <<'T'
#!/bin/bash
export PATH=/usr/lib/wsl/lib:$PATH
tmux has-session -t dash 2>/dev/null || {
  tmux new-session -d -s dash -x 220 -y 60 'btop'
  tmux split-window -h -t dash 'nvitop'
  tmux split-window -v -t dash 'compute-users'
  tmux select-layout -t dash main-vertical; }
exec tmux attach -t dash
T
chmod 755 /usr/local/bin/compute-dash
apt-get install -y -qq bc >/dev/null
compute-adduser demo "$(cat /home/admin/.ssh/authorized_keys)"
