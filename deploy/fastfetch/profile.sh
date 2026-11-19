# Show fastfetch once per interactive login (not for scp/sftp, spyder-kernel or other commands).
case $- in *i*) ;; *) return 0 2>/dev/null || exit 0 ;; esac
if [ -t 1 ] && [ -z "${CM_FETCH_SHOWN:-}" ] && [ ! -e "$HOME/.hushfetch" ] && command -v fastfetch >/dev/null; then
  export CM_FETCH_SHOWN=1
  fastfetch 2>/dev/null
  echo "  cm-top: dashboard · spyder-kernel <env>: start a kernel · touch ~/.hushfetch: hide this"
fi
