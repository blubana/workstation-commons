# 運算伺服器使用指南

這台伺服器（Intel Core Ultra 9 285K 24 核、128 GB RAM、NVIDIA RTX 5080 16 GB、Intel AI Boost NPU）提供給小組共用。你會拿到一個個人帳號，用 SSH 登入，在共用的 Python 環境裡跑程式，或讓自己電腦上的 Spyder 連過來執行。

---

## 1. 取得帳號

1. 在自己的電腦產生 SSH 金鑰（已經有了就跳過）：
   ```bash
   ssh-keygen -t ed25519
   ```
   Windows 用 PowerShell 或「命令提示字元」執行同一行即可。
2. 把**公鑰** `~/.ssh/id_ed25519.pub`（Windows：`C:\Users\<你>\.ssh\id_ed25519.pub`）交給管理員。
   **私鑰（沒有 `.pub` 的那個檔）絕對不要給任何人。**
3. 管理員建立帳號後會告訴你帳號名稱。伺服器只接受金鑰登入，沒有密碼。

## 2. 連線

| 位置 | 指令 |
|---|---|
| 區網內 | `ssh -p 2200 <帳號>@192.168.1.10` |
| 透過 Tailscale（遠端） | `ssh -p 2200 <帳號>@<tailscale-ip>` |
| 在伺服器本機（Windows） | 開「命令提示字元」輸入 `cm-top`，或 `ssh -p 2200 <帳號>@127.0.0.1` |

建議在自己電腦的 `~/.ssh/config` 加上：

```
Host compute
    HostName 192.168.1.10
    Port 2200
    User <帳號>
```

之後只要打 `ssh compute`。登入後會看到一個摘要畫面（fastfetch），列出硬體、你的記憶體額度、執行中的 kernel 和 NPU 狀態。不想看到就執行 `touch ~/.hushfetch`。

## 3. 硬體與分配規則

| 資源 | 規則 |
|---|---|
| CPU（24 核） | 所有人權重相同。沒人用時你可以用滿；多人同時用時平均分配。 |
| 記憶體 | 每人**保底 24 GB**（別人再忙也不會被搶走）、**軟上限 80 GB**（超過會被減速）、全部使用者合計上限 104 GB。 |
| GPU（RTX 5080，16 GB VRAM） | 不設限制，大家共用。VRAM 用完時其他人會配置失敗，用完請釋放（見 §6）。 |
| NPU（Intel AI Boost） | 透過排隊系統使用，一次只跑一個工作，使用者之間輪流（見 §7）。 |
| 磁碟 | 家目錄 `~/` 放程式與資料；`~/data` 是給你放資料集的私人資料夾。 |

> 記憶體壓力過大時，系統會優先終止**超過保底最多**的程式。長時間佔用大量記憶體前，先確認不會影響別人。

隨時可以執行 `cm-top` 看自己的用量、kernel 與硬體狀態（F1/F2/F3 切換版面，`h` 看即時硬體圖，`q` 離開）。

## 4. Python 環境

伺服器提供三個共用環境，已裝好常用的資料科學與機器學習套件：numpy、scipy、pandas、polars、matplotlib、seaborn、plotly、scikit-learn、statsmodels、xgboost、lightgbm、catboost、PyTorch（CUDA）、cupy、transformers、OpenVINO、opencv 等。

| 環境 | Python | 給哪個 Spyder |
|---|---|---|
| `sci` | 3.12 | **Spyder 6**（spyder-kernels 3.1） |
| `sci-spyder5` | 3.11 | **Spyder 5**（spyder-kernels 2.5） |
| `sci-legacy` | 3.11 | **Spyder 3.2.8** 與其他舊版（ipykernel） |

直接在終端機使用：

```bash
/opt/conda/envs/sci/bin/python my_script.py
# 或
source /opt/conda/bin/activate sci
python my_script.py
```

需要額外套件時，建立自己的環境（只有你看得到，不影響別人）。最省空間的做法是「沿用共用套件、再加自己的」，只佔幾十 MB：

```bash
/opt/conda/envs/sci/bin/python -m venv --system-site-packages ~/venvs/mywork
~/venvs/mywork/bin/pip install <套件>
~/venvs/mywork/bin/python my_script.py      # torch、cupy 等共用套件照樣可用
```

需要完全不同的 Python 版本或套件組合時，用 conda 建全新環境（存在 `~/.conda/envs`）：

```bash
source /opt/conda/bin/activate
conda create -n other python=3.10 numpy scipy
conda activate other
```

共用環境是唯讀的；要加進共用環境的套件請找管理員。

## 5. 用 Spyder 連到伺服器

1. 在伺服器啟動 kernel（依你的 Spyder 版本選環境）：
   ```bash
   ssh compute spyder-kernel sci          # Spyder 6
   ssh compute spyder-kernel sci-spyder5  # Spyder 5
   ssh compute spyder-kernel sci-legacy   # Spyder 3.2.8
   ```
   它會印出 `connection file: /home/<帳號>/.local/share/jupyter/runtime/kernel-....json`。
2. 把這個檔案複製到自己電腦：
   ```bash
   scp -P 2200 <帳號>@192.168.1.10:<上面印出的路徑> .
   ```
3. Spyder：**Consoles → Connect to an existing kernel**
   - Connection file：選剛才複製的 `.json`
   - 勾選 **This is a remote kernel (via SSH)**
   - Hostname：`<帳號>@192.168.1.10:2200`（遠端用 `<tailscale-ip>:2200`）
   - 選 **Keyfile**，指向你的私鑰 `id_ed25519`
4. 連上後在 Spyder 執行的程式碼都在伺服器上跑，檔案路徑也是伺服器上的路徑。

kernel 會一直在背景跑，關掉 Spyder 也不會停（登出也不會）。不用時請在 `cm-top` 的 Kernels 頁選取後按 `s` 停止，以釋放記憶體與 GPU。

## 6. 使用 GPU

```python
import torch
print(torch.cuda.is_available(), torch.cuda.get_device_name(0))   # True NVIDIA GeForce RTX 5080
x = torch.randn(8192, 8192, device="cuda")
y = x @ x

import cupy as cp                  # numpy 介面的 GPU 版本
a = cp.random.rand(4096, 4096)
b = cp.linalg.inv(a)
```

- 查看 GPU 使用狀況：`nvidia-smi`，或 `cm-top` 的 Overview/Hardware 頁。
- 用完釋放 VRAM，讓別人也能用：
  ```python
  del x, y
  torch.cuda.empty_cache()
  cp.get_default_memory_pool().free_all_blocks()
  ```
  最簡單的方法是重啟或停止 kernel。

## 7. 使用 NPU（Intel AI Boost）

NPU 適合「同一個運算重複做很多次」的情況，例如大量矩陣運算或固定形狀的神經網路推論。它用 FP16 計算，精度比 CPU/GPU 低（誤差約 1e-3），**輸入形狀必須固定**。任何環境都可以 `import cm_npu`：

```python
import numpy as np, openvino as ov, cm_npu
from openvino import opset13 as ops

n = 512
a = ops.parameter([n, n], np.float32, name="a")
b = ops.parameter([n, n], np.float32, name="b")
model = ov.Model([ops.tanh(ops.matmul(a, b, False, False))], [a, b])

A = np.random.rand(n, n).astype(np.float32) / n
B = np.random.rand(n, n).astype(np.float32)
out = cm_npu.run(model, {"a": A, "b": B})          # 預設 device="NPU"
print(list(out.values())[0].shape, cm_npu.last_timing)
```

- PyTorch 模型：`cm_npu.run(ov.convert_model(torch_model, example_input=x), {...})`。
- 比較速度或精度：`device="CPU"` 或 `device="GPU.0"`（Intel 內顯）跑同一個模型。
- 第一次呼叫會編譯模型（約 1 秒），之後同一個模型會快取，每次只要幾毫秒。
- 查看佇列：`cm_npu.status()`。每人最多排 4 個工作；請求大小上限 256 MB。
- 一般的 numpy 運算請直接用 CPU 或 GPU；NPU 只在上述情況才划算。

## 8. 常見問題

| 狀況 | 處理 |
|---|---|
| `Permission denied (publickey)` | 金鑰不對或還沒被加入，確認用的是交給管理員的那把金鑰（`ssh -i ~/.ssh/id_ed25519 ...`）。Windows 若出現 `UNPROTECTED PRIVATE KEY FILE`，私鑰權限太寬，請管理員協助。 |
| 程式突然變很慢 | 可能超過 80 GB 軟上限被減速，看 `cm-top` 的記憶體條。 |
| 程式被終止（Killed） | 記憶體壓力過大時被系統終止。減少資料量、分批處理，或請管理員調整策略。 |
| `CUDA out of memory` | 其他人或你自己的舊 kernel 佔用 VRAM，用 `nvidia-smi` 查看，停止不用的 kernel。 |
| NPU 回 `429` / `503` | 你排了太多工作，或排隊太久，稍後再試。 |
| Spyder 連不上 | 確認 kernel 還在跑（`cm-top` → Kernels），connection file 是最新的那個，Hostname 有帶 `:2200`。 |
| 想知道伺服器狀態 | `cm-top`、`nvidia-smi`、`fastfetch`。 |

## 9. 使用守則

- 不要把私鑰分享給別人，一人一個帳號。
- 長時間大量使用 GPU 或記憶體前先告知其他人。
- 不用的 kernel 請停止。
- 重要資料自行備份，伺服器不保證備份。
- 有問題或需要新套件，請聯絡管理員。
