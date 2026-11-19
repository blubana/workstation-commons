#!/bin/bash
# Install fastfetch (pinned + sha256-verified) with a shared config shown at interactive login.
# Run as root. Users opt out with `touch ~/.hushfetch`.
set -euo pipefail
VER=2.71.0
SHA256=6dc0b71c03e58f9f981a6b3c62b4fa3fb30b9d5ee9fa783302c91ea4c2a3600e
SRC=$(cd "$(dirname "$0")" && pwd)
if [ "$(fastfetch --version 2>/dev/null | awk '{print $2}')" != "$VER" ]; then
  tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
  curl -fsSL -o "$tmp/ff.deb" "https://github.com/fastfetch-cli/fastfetch/releases/download/$VER/fastfetch-linux-amd64.deb"
  echo "$SHA256  $tmp/ff.deb" | sha256sum -c --quiet
  apt-get install -y -q "$tmp/ff.deb" >/dev/null
fi
install -d -m755 /etc/xdg/fastfetch /usr/local/libexec/compute-fetch
install -m644 "$SRC/fastfetch/config.jsonc" /etc/xdg/fastfetch/config.jsonc
install -m755 "$SRC/fastfetch/compute-info" /usr/local/libexec/compute-fetch/compute-info
install -m644 "$SRC/fastfetch/connect.conf" /etc/compute-mgr/connect.conf
install -m644 "$SRC/fastfetch/profile.sh" /etc/profile.d/zz-fastfetch.sh
fastfetch --version
