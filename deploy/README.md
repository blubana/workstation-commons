# server 運算環境使用說明

連線：`ssh -p 2200 <帳號>@192.168.1.10`（限區網或 tailnet，只接受金鑰登入）

## 環境（共用 conda，位於 /opt/conda/envs）
| env | Python | 對應 Spyder |
|---|---|---|
| sci | 3.12 | 6.x（spyder-kernels 3.1） |
| sci-spyder5 | 3.11 | 5.x（spyder-kernels 2.5） |
| sci-legacy | 3.11 | 3.2.x（純 ipykernel） |
都內含 numpy、scipy、pandas、matplotlib、numba、cupy（RTX 5080）。
想建自己的環境：`conda create -n myenv ...`，會建在 ~/.conda/envs。

## Spyder 連線（所有版本通用）
1. 在 server 啟動 kernel：`ssh -p 2200 <帳號>@192.168.1.10 spyder-kernel sci-legacy`（Spyder 3.2.8 用 sci-legacy）
2. 把印出的 connection file 複製到本機：`scp -P 2200 <帳號>@192.168.1.10:<路徑> .`
3. Spyder → Consoles → Connect to an existing kernel → 選該 json，勾選「This is a remote kernel (via SSH)」
   Hostname：`<帳號>@192.168.1.10:2200`，用私鑰驗證。
注意：程式讀寫的是 server 上的路徑，資料放 ~/data。kernel 登出後仍會保留（linger），用完請關掉。

## 限制
每人最多 28 GB RAM、16 核心。GPU 16 GB 大家共用，請勿長時間占滿。

## 管理（admin）
- 新增使用者：`sudo compute-adduser <name> "<ssh公鑰>"`
- 監控面板：`compute-dash`（btop + nvitop + 各使用者用量，tmux：Ctrl-b d 離開）
