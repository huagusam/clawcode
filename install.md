# Claw Code 安装指南

> 面向 Windows 用户的一键安装说明。整个安装过程只需下载、解压、双击一个 bat。

## 一、下载

从 [GitHub Releases](https://github.com/huagusam/clawcode/releases/latest) 获取两个文件：

| 文件 | 下载地址 | 说明 |
|---|---|---|
| `Config_methods.zip` | [点此下载](https://github.com/huagusam/clawcode/releases/download/v0.2.2.1/Config_methods.zip) | **完整安装包**，包含 claw.exe、Git、fd、rg、配置文件与安装脚本 |
| `claw.exe` | [点此下载](https://github.com/huagusam/clawcode/releases/download/v0.2.2.1/claw.exe) | 单独的主程序（可选，安装包已内含） |

> 推荐直接下载 **`Config_methods.zip`** 一个文件即可完成全部安装。

## 二、解压

1. 右键点击 `Config_methods.zip` → **全部解压 / Extract All**（Windows 自带解压；没有则安装 [7-Zip](https://www.7-zip.org/)）
2. 解压后得到 `Install_Config_methods` 文件夹，内含：
   - `claw.exe` — 主程序
   - `Git.7z` — Git Bash 离线安装包
   - `fd.exe` / `rg.exe` — 搜索工具
   - `.claw/` — 配置文件目录
   - `install_claw.bat` — **一键安装脚本**

> 注意：文件夹路径中**不要包含中文**，例如放在 `D:\claw\Install_Config_methods`。

## 三、一键安装

1. 进入解压出的 `Install_Config_methods` 文件夹
2. **双击 `install_claw.bat`**，按提示同意管理员权限（UAC 弹窗点"是"）
3. 脚本会自动完成：

| 步骤 | 内容 |
|---|---|
| 1/5 | 检测 Git Bash：已安装则跳过，未安装则解压 `Git.7z` 到 `C:\Program Files\Git` |
| 2/5 | 把 `fd.exe`、`rg.exe` 复制到 `C:\Program Files\Git\bin` |
| 3/5 | 把 `claw.exe` 复制到 `C:\Users\你的用户名\.local\bin`，并在桌面创建 `claw` 快捷方式 |
| 4/5 | 把 `.claw` 配置文件夹复制到 `C:\Users\你的用户名\.claw`（覆盖旧配置） |
| 5/5 | 把 `C:\Program Files\Git\bin` 和 `.local\bin` 加入系统 PATH |

看到 **"Installation finished"** 即安装成功。

## 四、开始使用

1. **重新打开**一个新的终端窗口（cmd / PowerShell / Windows Terminal），让 PATH 生效
2. 双击桌面上的 **`claw`** 快捷方式，或在终端输入 `claw` 回车
3. 首次使用请配置 API：编辑 `C:\Users\你的用户名\.claw\.env`，填入你的 API Key 和模型：

```env
ANTHROPIC_BASE_URL=https://api.anthropic.com
ANTHROPIC_API_KEY=sk-ant-xxxxxxxx
ANTHROPIC_MODEL=claude-sonnet-4-20250514
```

> 使用本地模型（LM Studio / llama.cpp / Ollama）：`ANTHROPIC_BASE_URL` 只需写服务器地址（**不要**带 `/v1`，claw 会自动拼接 `/v1/messages`）。端口随服务而异：LM Studio `1234`、llama-server `8080`、Ollama `11434`。

## 五、常见问题

| 问题 | 解决方法 |
|---|---|
| 双击 bat 后窗口一闪而过 | 右键 `install_claw.bat` → 以管理员身份运行 |
| 提示 7-Zip 未找到 | 安装 [7-Zip](https://www.7-zip.org/) 后重新运行脚本 |
| 输入 `claw` 提示找不到命令 | 确认 PATH 已生效，或重新打开终端再试 |
| 桌面没有快捷方式 | 检查安装日志，或手动创建指向 `C:\Users\你的用户名\.local\bin\claw.exe` 的快捷方式 |
| 需要卸载 | 删除 `C:\Users\你的用户名\.local\bin\claw.exe`、`C:\Users\你的用户名\.claw` 和桌面快捷方式即可 |

## 六、从源码构建（可选）

需要 Rust + MSVC + Clang-CL 环境，详见项目 [README](README.md)。

## License

MIT
