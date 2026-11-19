@echo off
rem cm-top for Windows: opens the TUI inside the compute WSL over SSH.
rem Linux account = %CM_USER% if set, else the Windows user name in lower case.
setlocal
if defined CM_USER (set "U=%CM_USER%") else (for /f "usebackq delims=" %%a in (`powershell -NoProfile -Command "$env:USERNAME.ToLower()"`) do set "U=%%a")
"%SystemRoot%\System32\OpenSSH\ssh.exe" -t -p 2200 -o StrictHostKeyChecking=accept-new %U%@127.0.0.1 cm-top %*
if errorlevel 255 (
  echo.
  echo cm-top: could not log in as "%U%" on the compute server.
  echo  - Ask an admin to create Linux account "%U%" with your public key ^(%USERPROFILE%\.ssh\id_ed25519.pub^)
  echo  - No key yet? run:  ssh-keygen -t ed25519
  echo  - Different Linux name? run:  setx CM_USER yourname   ^(then open a new window^)
)
