set -e
systemctl unmask ssh.socket
mkdir -p /etc/systemd/system/ssh.socket.d
# Mirrored WSL needs the IPv4 wildcard listener. setup-win.ps1 restricts ingress.
printf '[Socket]\nListenStream=\nListenStream=0.0.0.0:2200\n' > /etc/systemd/system/ssh.socket.d/port.conf
systemctl daemon-reload
systemctl enable ssh.socket
systemctl restart ssh.socket
dpkg --configure -a
ss -ltnp | grep 2200
