# 第一次連線：一步一步設定

照順序做，大約 10 分鐘。每一步都有「做完應該看到什麼」，對不上就停下來問管理員。

## 步驟 1：打開終端機
- **Windows**：開始功能表搜尋「PowerShell」並開啟。
- **macOS**：開啟「終端機」(Terminal)。
- **Linux**：開啟任一終端機。

輸入 `ssh -V` 按 Enter。
✅ 應該看到 `OpenSSH_...`。Windows 若顯示找不到指令：設定 → 系統 → 選用功能 → 新增「OpenSSH 用戶端」。

## 步驟 2：檢查是不是已經有金鑰
```
ls ~/.ssh
```
看到 `id_ed25519` 和 `id_ed25519.pub` 兩個檔案 → 已經有了，跳到步驟 4。

## 步驟 3：產生金鑰
```
ssh-keygen -t ed25519
```
- 問存放位置：直接按 Enter。
- 問 passphrase（金鑰密碼）：建議輸入一組自己記得的密碼，按 Enter，再輸入一次。畫面不會顯示字元，這是正常的。

✅ 最後會出現一個方塊圖案，`~/.ssh` 裡多了 `id_ed25519`（私鑰）和 `id_ed25519.pub`（公鑰）。

## 步驟 4：取得公鑰
```
cat ~/.ssh/id_ed25519.pub
```
✅ 會印出一行，像這樣：`ssh-ed25519 AAAAC3Nza...很長... 你的名字@電腦`

把**整行**複製，用 email 或訊息傳給管理員。
⚠️ 只傳 `.pub` 這個。**沒有 .pub 的私鑰永遠不要給任何人**，管理員也不需要它。

## 步驟 5：等管理員建立帳號
管理員會回覆你的**帳號名稱**。

## 步驟 6：設定捷徑
在終端機輸入（把 `你的帳號` 換掉）：

**macOS / Linux**
```
cat >> ~/.ssh/config <<'EOF'
Host compute
    HostName 192.168.1.10
    Port 2200
    User 你的帳號
EOF
```
**Windows PowerShell**
```
Add-Content $HOME\.ssh\config "Host compute`n    HostName 192.168.1.10`n    Port 2200`n    User 你的帳號"
```
在外面（非實驗室網路）連線時，把 `192.168.1.10` 換成 `<tailscale-ip>`（需先登入 Tailscale）。

## 步驟 7：第一次登入
```
ssh compute
```
- 第一次會問 `Are you sure you want to continue connecting?`，輸入 `yes`。
- 有設 passphrase 會要你輸入。

✅ 看到伺服器資訊畫面和 `你的帳號@SERVER:~$` 就成功了。畫面黃色的部分是你專屬的連線指令，之後忘記了就登入看，或輸入 `cm-info`。輸入 `exit` 離開。

## 步驟 7.5：安裝檔案傳輸工具 termscp（建議）
- macOS：`brew install termscp`
- Windows：`winget install termscp`
- Linux：`cargo install termscp`

執行 `termscp sftp://你的帳號@192.168.1.10:2200`，左邊是你的電腦、右邊是伺服器，選檔案按 `Space` 傳送，`q` 離開。

## 步驟 8：連接 Spyder（選用）
1. 啟動 kernel：`ssh compute spyder-kernel <你的 Spyder 版本>`，例如 `spyder-kernel 6.1`、`spyder-kernel 5.4`（Spyder → Help → About 查版本）。記下印出的 `connection file:` 路徑。
2. 下載該檔：`scp compute:<剛才的路徑> .`
3. Spyder → **Consoles → Connect to an existing kernel**
   - 選剛下載的 `.json`
   - 勾 **This is a remote kernel (via SSH)**
   - Spyder 4–6：Hostname `192.168.1.10`、Port `2200`、Username `你的帳號`（Spyder 3.x 才是 Hostname 填 `你的帳號@192.168.1.10:2200`）
   - Keyfile：選 `~/.ssh/id_ed25519`（私鑰，留在你電腦上）
4. 按 OK。✅ 在 Console 輸入 `import socket; socket.gethostname()`，顯示 `SERVER` 代表程式在伺服器上執行。

用完記得停 kernel：登入後執行 `cm-top`，Kernels 頁選取按 `s`。

## 出問題時
| 訊息 | 原因 |
|---|---|
| `Permission denied (publickey)` | 管理員還沒加入你的公鑰，或你傳錯檔案 |
| `UNPROTECTED PRIVATE KEY FILE` | Windows 私鑰權限太寬，請管理員協助 |
| `Connection timed out` | 不在實驗室網路，改用 Tailscale 位址 |
