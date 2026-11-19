# Show fastfetch once per interactive login (not for scp/sftp, spyder-kernel or other commands).
case $- in *i*) ;; *) return 0 2>/dev/null || exit 0 ;; esac
if [ -t 1 ] && [ -z "${CM_FETCH_SHOWN:-}" ] && [ ! -e "$HOME/.hushfetch" ] && command -v fastfetch >/dev/null; then
  export CM_FETCH_SHOWN=1
  fastfetch 2>/dev/null
  echo "  guide: less /opt/compute/README.md · dashboard: cm-top · show again: cm-info · hide: touch ~/.hushfetch"
fi
cm-info() { fastfetch; }
