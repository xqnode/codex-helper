# Codex Helper

> 轻量托盘 / 菜单栏工具 — 让 **OpenAI Codex CLI** 一键切换到 DeepSeek、通义千问、智谱、Kimi、MiniMax 等国产大模型。

> **零门槛设计**：双击安装 → 自动配置 → 托盘右键选模型 → 开始用 Codex。
> 全程不需要打开终端、不需要改配置文件、不需要懂技术。

---

## 界面预览

右键系统托盘图标，即可切换模型、打开设置或同步配置：

![系统托盘菜单](assets/tray-menu.png)

托盘 →「设置…」打开 API Key 配置窗口（厂商 Chip 切换、Base URL、测试连接）：

![设置窗口](assets/settings-window.png)

---

## 这是给谁用的？

- **小白用户**：第一次接触 Codex，只想用 DeepSeek 替代 OpenAI 省钱
- **怕折腾的用户**：看到 `~/.codex/config.toml` 就头大
- **多账号用户**：在 DeepSeek、通义、Moonshot 之间快速切换

如果你已经是命令行高手，也可以走 [高级模式](#高级模式cli)。

---

## 一图看懂

```
┌──────────────────────────────────────────────────────────┐
│  系统托盘  ┌───┐                                          │
│            │ ⚡ │  ← 右键点这里                             │
│            └───┘                                          │
│       × API Key：未配置                                     │
│       × 连接：需先配置 Key                                  │
│       ─────────────────                                    │
│       切换模型 · DeepSeek · pro  >   ← 厂商 → 具体型号      │
│       ─────────────────                                    │
│       常用                                                 │
│         设置…                                              │
│         重新同步配置 / 检测连接 / 请求日志…                   │
│       更多  → 配置文件夹、切回 OpenAI 官方…                   │
│       ─────────────────                                    │
│       退出 Codex Helper                                    │
└──────────────────────────────────────────────────────────┘
                       ↓ 托盘里切换模型
┌──────────────────────────────────────────────────────────┐
│  Codex Desktop / CLI  →  Codex Helper 代理  →  模型 API   │
└──────────────────────────────────────────────────────────┘
```

**就这么简单：装上 → 托盘里填 Key、选模型 → 在 Codex 里用。**

---

## 三步上手

### 第 1 步：下载安装

去 [Releases 页面](https://github.com/xqnode/codex-helper/releases) 下载：

| 平台 | 类型 | 文件 | 操作 |
|------|------|------|------|
| **Windows** | 安装版 | `CodexHelper-x.x.x-Setup.exe` | 双击 → 一路下一步 |
| **Windows** | 便携版 | `CodexHelper-x.x.x-win64.zip` | 解压后双击 `codex-helper.exe` |
| **macOS** | DMG | `CodexHelper-x.x.x-macos.dmg` | 拖入「应用程序」→ 从启动台打开（菜单栏图标，无 Dock 图标） |

> **macOS 说明**：v0.1.0 起支持菜单栏托盘、设置窗口与请求日志。DMG 需在 Mac 上自行构建（见下方「macOS 构建」）；公开发布前还需 Apple 代码签名与公证，否则 Gatekeeper 可能拦截。Linux 仍仅 CLI 模式（`codex-helper start --no-tray`）。

Windows 安装版会自动：可选开机启动、写入 Codex 配置、注册系统托盘。

### 第 2 步：填 API Key（首次启动自动弹出）

首次启动时，Codex Helper 会**自动检测**你的环境：

- ✅ 已装 Codex？— 自动写入代理配置
- ✅ 已有 `DEEPSEEK_API_KEY` 环境变量？— 自动读取
- ✅ 都没有？— 弹出引导窗口：

```
┌────────────────────────────────────────────────┐
│  欢迎使用 Codex Helper                          │
│                                                │
│  请选择一个模型：                                │
│  ● DeepSeek（推荐，性价比最高）                  │
│  ○ 通义千问                                     │
│  ○ Moonshot                                    │
│                                                │
│  粘贴你的 API Key:                              │
│  ┌──────────────────────────────────────────┐  │
│  │ sk-...                                   │  │
│  └──────────────────────────────────────────┘  │
│  👉 还没有 Key？[点这里申请 DeepSeek Key]      │
│                                                │
│  [测试连接]                    [完成]            │
└────────────────────────────────────────────────┘
```

### 第 3 步：打开 Codex 就能用

```bash
codex
```

完成。**不需要任何额外配置。**

切换模型？**托盘切换厂商通常无需重启**（详见 [常见问题](#faq-model-switch)）。

---

## 小白友好设计

我们把所有「技术门槛」都铲平了：

| 你担心的事 | Codex Helper 怎么做 |
|-----------|---------------------|
| 不知道 Codex 装没装 | 启动时自动检测，没装会给下载链接 |
| 不知道去哪申请 Key | 每个模型旁边都有「点此申请」直达官网 |
| 不知道 Key 填对没 | 输入框实时校验格式，填错变红 |
| 怕改坏配置文件 | 全程 GUI，自动备份 10 份历史 |
| 报错看不懂英文 | 所有错误翻译成中文 + 给出具体建议 |
| 不知道当前用的哪个模型 | 托盘图标颜色区分（蓝=DeepSeek，绿=通义...）+ 鼠标悬停显示 |
| 切换后没生效 | 自动检测 Codex 进程，提示「请重启 Codex」并提供一键操作 |
| 想回到 OpenAI 官方 | 托盘菜单 → 「恢复官方登录」一键完成 |
| 后台代理怎么停 | 退出托盘程序 = 自动停代理；下次开机自动启动 |

---

## 友好错误提示示例

❌ 不好：
```
Error: 401 Unauthorized
```

✅ Codex Helper：
```
┌────────────────────────────────────────────────┐
│  ⚠ DeepSeek API Key 无效                       │
│                                                │
│  可能原因：                                      │
│  • Key 被复制时多了空格或换行                    │
│  • Key 已过期或被删除                            │
│  • DeepSeek 账户余额不足                         │
│                                                │
│  [重新填写 Key]    [打开 DeepSeek 控制台]        │
└────────────────────────────────────────────────┘
```

---

## 支持的模型

| 模型 | 推荐场景 | 申请 Key |
|------|---------|---------|
| **DeepSeek** | 性价比最高，推荐首选 | [platform.deepseek.com](https://platform.deepseek.com/) |
| **通义千问** | 阿里生态，国内速度快 | [dashscope.aliyun.com](https://dashscope.aliyun.com/) |
| **Moonshot** | 长上下文优秀 | [platform.moonshot.cn](https://platform.moonshot.cn/) |
| **智谱 GLM** | 中文能力强 | [bigmodel.cn](https://www.bigmodel.cn/) |
| **中转站** | 任何 OpenAI 兼容 API | — |

> 中转站端点也是在 GUI 里填，不用编辑配置文件。

---

## 设置窗口（迷你 GUI）

托盘右键 → 「设置」打开，包含：

- **模型管理**：添加 / 删除 / 编辑模型预设
- **API Key 管理**：所有 Key 集中管理，掩码显示
- **代理设置**：本地监听 `127.0.0.1:25543`（固定端口，默认无需改）
- **开机启动**：开关
- **导出 / 导入**：备份你的配置到其他电脑
- **关于**：版本、检查更新、查看日志

整个窗口预计 < 500 行代码，绝不臃肿。

---

## 工作原理（可跳过）

```
┌─────────────┐   1. Codex 永远连本地代理   ┌────────────────┐
│  Codex CLI  │ ──────────────────────────► │  Codex Helper  │
│             │   http://127.0.0.1:25543/v1     │   (托盘进程)    │
└─────────────┘                              └────────┬───────┘
                                                      │
                              2. 代理根据你的选择转发   │
                                                      ▼
                              ┌───────────┬──────────┬──────────┐
                              ▼           ▼          ▼          ▼
                         DeepSeek      通义        Moonshot   中转站
```

- Codex 的 `~/.codex/config.toml` 一次性写好，永不再改
- 切换模型 = 切换代理的转发目标，**Codex 完全无感知**
- 代理自动处理 Responses API 与 Chat Completions 的格式转换

---

## 高级模式（CLI）

如果你喜欢命令行，也可以用 CLI 控制托盘程序：

```bash
codex-helper use deepseek      # 切换模型
codex-helper status            # 查看当前状态
codex-helper test              # 测试当前模型连通性
codex-helper list              # 列出所有模型
codex-helper restore-openai    # 恢复 OpenAI 官方
codex-helper doctor            # 一键诊断
```

CLI 和托盘共享同一个后端，命令立即反映到托盘图标。

---

## 安装包做了什么

为了真正「双击下一步」，安装包会自动完成：

1. 安装主程序到 `Program Files\CodexHelper\`（Windows）
2. 添加开机启动项（可在设置中关闭）
3. 注册 `codex-helper://` Deep Link（用于 Key 一键导入）
4. **自动备份** 现有的 `~/.codex/config.toml` 到 `~/.codex-helper/backups/`
5. 注入代理配置到 `~/.codex/config.toml`
6. 在系统托盘启动主程序
7. 弹出首次引导窗口

**全程无需打开终端。**

---

## 卸载也很干净

Windows「设置 → 应用」卸载，或运行安装目录下的卸载程序，将：

- ✅ 还原 `~/.codex/config.toml` 到安装前状态（从备份）
- ✅ 移除开机启动项
- ✅ 询问是否保留 `~/.codex-helper/` 配置目录

**不残留任何东西。**

---

## 与 CC Switch 的区别

| | CC Switch | Codex Helper |
|---|-----------|--------------|
| 定位 | 7 种 AI 工具全能管理器 | **专注 Codex** |
| 体积 | 桌面应用 ~50MB | **托盘 ~10MB** |
| 上手成本 | 需要理解 Provider 概念 | **零概念，选模型就行** |
| 适合人群 | 多工具高级用户 | **小白 + 只用 Codex 的人** |
| 学习曲线 | 中等 | **几乎为零** |

如果你只用 Codex，Codex Helper 更轻、更专、更省心。

---

## 数据存储

| 路径 | 内容 |
|------|------|
| `~/.codex-helper/config.json` | 当前模型、代理端口（默认 **25543**）等设置 |
| `~/.codex-helper/keys.enc` | **加密存储**的 API Keys（不明文） |
| `~/.codex-helper/backups/` | Codex 配置自动备份（保留 10 份） |
| `~/.codex-helper/logs/` | 运行日志（出问题时上传） |

**所有数据仅在本地，不上传任何服务器。**

---

## 技术栈（开发者）

- **核心**：Rust（小体积、跨平台、单 exe）
- **托盘**：`tray-icon` + `tao`（无需 Electron/Tauri 全套）
- **设置窗口**：原生 webview（仅在打开时加载，~3MB）
- **代理**：`hyper` + `tokio`
- **打包**：Windows `scripts/build-all.bat` / `build-zip.bat`（Inno Setup + zip）；macOS `scripts/build-macos-release.sh` → `.app` + DMG（Linux AppImage 规划中）

预计单文件 < 10MB，内存占用 < 30MB。

---

## 本地开发（开发者）

安装 [Rust](https://rustup.rs/) 后，在项目根目录：

```bash
git clone https://github.com/xqnode/codex-helper.git
cd codex-helper
```

### 首次

```bash
cargo run -- init    # 初始化配置，写入 ~/.codex/config.toml
cargo run            # 默认即 start：启动托盘 + 本地代理（Windows / macOS）
```

### 常用命令

| 目的 | 命令 |
|------|------|
| 启动托盘（默认） | `cargo run` 或 `cargo run -- start` |
| 只跑代理、不要托盘 | `cargo run -- start --no-tray` |
| 打开设置窗口 | `cargo run -- settings` |
| 查看状态 / 诊断 | `cargo run -- status` / `cargo run -- doctor` |
| 切换模型 / 测试 | `cargo run -- use deepseek` / `cargo run -- test` |

`cargo run -- <子命令>` 与安装后的 `codex-helper <子命令>` 等价；CLI 与托盘共享同一后端。

### Debug 与 Release

- **`cargo run`（debug）**：Windows 会保留**控制台窗口**，`println!` 与错误直接可见，适合日常开发。
- **`cargo run --release`**：更接近正式包；Windows Release 构建会隐藏控制台（无黑框）。

可选：提高日志级别便于排查代理与托盘问题：

```bash
# PowerShell
$env:RUST_LOG = "codex_helper=debug"
cargo run

# bash / zsh
RUST_LOG=codex_helper=debug cargo run
```

### 开发时注意

1. **先关掉已在跑的实例**：默认端口 `25543`。若已安装正式版或另一个 `codex-helper` 在跑，再 `cargo run` 会提示「已在运行」或端口占用；任务管理器结束多余的 `codex-helper` 后再启动。
2. **改代码后**：`Ctrl+C` 停掉当前进程，再重新 `cargo run`（无热重载）。
3. **Linux**：托盘暂不支持，开发时用 `cargo run -- start --no-tray`。

Windows 打包：`scripts/build-all.bat` / `build-zip.bat`；macOS 见下方「macOS 构建」。

---

## 交流群

扫码加入 **AI 交流群**（企业微信），交流 Codex 使用心得、反馈问题：

<p align="center">
  <img src="assets/ai-group-qr.png" alt="AI 交流群二维码" width="240">
</p>

---

## macOS 构建（开发者）

在 Mac 上安装 [Rust](https://rustup.rs/) 后：

```bash
git clone https://github.com/xqnode/codex-helper.git
cd codex-helper
chmod +x scripts/build-macos-release.sh
./scripts/build-macos-release.sh
```

产物在 `dist/`：

- `Codex Helper.app` — 菜单栏应用（`LSUIElement`，无 Dock 图标）
- `CodexHelper-x.x.x-macos.dmg` — 拖入「应用程序」安装

首次使用：

```bash
open -a "Codex Helper"   # 或从 DMG 安装后从启动台打开
# 菜单栏 → 设置… → 填 API Key
codex-helper doctor      # 可选：诊断
```

API Key 写入 `~/.codex/.env`（macOS 不依赖 Windows 的 `setx`）。代理默认 `http://127.0.0.1:25543/v1`。

可选：放置 `assets/codex-helper.png`（512×512）后重新打包，脚本会自动生成 `.icns`。

---

## 贡献

欢迎 Issue 和 PR，尤其欢迎：

- 新模型预设（附 base_url + 测试通过截图）
- 中文报错文案优化
- 小白用户的反馈（你卡在哪一步了？）

---

## 常见问题

**Q：Mac 上找不到图标？**
A：Codex Helper 是**菜单栏应用**（右上角状态栏），不会在 Dock 显示。点菜单栏闪电图标 →「设置…」配置 Key。若被系统隐藏，在「系统设置 → 控制中心 → 菜单栏」中调整。

**Q：Mac 提示「无法打开，因为无法验证开发者」？**
A：当前 DMG 尚未公证。可右键 App →「打开」→ 再次确认；或自行从源码 `cargo build --release` 运行。公开发布需 Apple Developer 账号做 codesign + notarize。

**Q：装上后 Codex 怎么没反应？**
A：看任务栏右下角是否有 Codex Helper 托盘图标（圆角蓝底 + 金色闪电）。没有的话，开始菜单搜「Codex Helper」启动一次；便携版请双击 `codex-helper.exe`（会自动启动托盘）。

**Q：便携版 zip 解压后双击 exe 没反应？**
A：新版已支持双击直接启动。若仍无托盘，在 PowerShell 进入解压目录执行：

```powershell
.\codex-helper.exe start
```

首次使用建议先执行 `.\codex-helper.exe init`，再 `start`。

**Q：安装版和 zip 便携版有什么区别？**
A：功能相同，都是同一个 `codex-helper.exe`。安装版会写入开始菜单、支持卸载程序，并可选开机自启；zip 解压即用，适合不想装软件的用户。

**Q：升级或重装后，图标还是旧的（蓝色圆形 / 模糊）？**
A：多半是 **Windows 图标缓存** 没刷新——同一路径反复覆盖安装时，资源管理器和任务栏可能仍显示旧图标，即使 exe 已是新版。安装包结束时会自动执行 `ie4uinit.exe -show`；若仍不对，在 PowerShell 执行：

```powershell
ie4uinit.exe -show
taskkill /IM explorer.exe /F; start explorer.exe
```

仍无效可**注销或重启电脑**。正确图标应为**圆角蓝底渐变 + 金色闪电**（与设置页左上角一致）。

**Q：设置窗口打不开？**
A：需先启动托盘代理。托盘 →「设置…」，或先运行 `codex-helper start`，再执行 `codex-helper settings`。若提示端口占用，说明已有实例在跑，在任务栏找到托盘图标即可。

**Q：Codex 报 `error sending request` / `502` / 连不上本地代理？**
A：Codex 通过固定地址 `http://127.0.0.1:25543/v1` 访问 Helper。请依次检查：

1. 任务栏是否有 Codex Helper 托盘图标（没有则启动 `codex-helper.exe start`）
2. 浏览器打开 [http://127.0.0.1:25543/health](http://127.0.0.1:25543/health) 应返回正常
3. 托盘 → **重新同步配置**，然后**完全退出并重启 Codex Desktop**
4. 运行 `codex-helper doctor`，确认 config.toml 与 config.json 端口均为 **25543**

若从旧版升级后 config 里仍是随机端口（如 18063），删除 `%USERPROFILE%\.codex-helper\config.json` 后重新 `codex-helper init`，或托盘重新同步配置。

- `502 Bad Gateway`：Helper 已连上，多为 API Key / 网络 / 中转站问题 → 设置里「测试连接」
- `error sending request`：Helper 未运行或端口不一致 → 按上面 1–4 步排查

**Q：任务管理器里有两个 `codex-helper.exe`？**
A：可能同时跑了**安装版 + zip 便携版**，或旧开机自启路径未删。任务管理器 → 右键 → 打开文件所在位置，保留一份即可；检查 `shell:startup` 里是否有多余快捷方式。

**Q：下载后 Windows 提示「已保护你的电脑」？**
A：安装包未数字签名，属正常情况。点「更多信息」→「仍要运行」。仅建议从 [GitHub Releases](https://github.com/xqnode/codex-helper/releases) 下载。

**Q：可以同时用 OpenAI 官方和 DeepSeek 吗？**
A：可以。托盘 → 更多 →「切换回 OpenAI 官方」，或 CLI 执行 `codex-helper restore-openai`。

**Q：如何清除所有配置？**
A：托盘 → 设置 → 右上角「清除所有配置」。会删除 API Key、厂商选择与请求日志；之后需重新填 Key 并重启 Codex Desktop。

**Q：切换模型需要重启 Codex 吗？**

<a id="faq-model-switch"></a>

**Codex Helper 本身不用重启**，托盘切换后代理会一直运行。

| 操作 | 需要重启 Codex？ |
|------|------------------|
| 托盘切换厂商（DeepSeek → 千问 等） | 通常**不需要**，新开一条对话即可 |
| 设置里改具体型号或 API Key | **建议**完全退出并重新打开 Codex Desktop |
| 切换后仍不对 | 完全退出 Desktop 再打开，或托盘 →「重新同步配置」 |

说明：

- **托盘切换厂商**：会立刻热更新代理、写入 `~/.codex/config.toml`、同步模型目录和 Desktop 会话库，下一条消息即走新厂商。
- **设置里改型号**（如 V4 Flash → V4 Pro）：保存后配置已更新，但 Desktop 可能缓存旧 UI，因此建议重启。
- **在 Codex 里点选模型**：选项来自 Helper 写入的目录，但**实际调用以 Helper 当前配置为准**；改型号请走 **托盘 → 设置… → 保存**。

若切换后没生效：先在 Codex 里**新开对话**试一次；仍不对则**完全退出 Codex Desktop**（任务栏右键退出，不要只关窗口）。

**Q：中转站 Base URL 怎么填？**
A：填 OpenAI 兼容网关地址，需带 `/v1`，例如 `http://your-host:8080/v1`。官方厂商的 Base URL 在设置里只读，无需修改。

**Q：一键诊断怎么用？**
A：PowerShell 或 cmd 执行 `codex-helper doctor`，会检查配置目录、Codex 配置、API Key、环境变量与代理是否在运行。

**Q：会不会偷我的 API Key？**
A：源代码开源，Key 存于本地 `%USERPROFILE%\.codex-helper\`，请求只发往你选的官方端点。

**Q：付费吗？**
A：完全免费，MIT 协议。模型 API 费用付给各模型厂商。

**Q：Codex Desktop 里 Computer Use / Browser Use 报「Node REPL 工具不可用」？**
A：Computer Use 依赖 Codex 本地的 `node_repl`（需 `[features] js_repl = true`）。Codex Helper 每次同步配置时会自动打开该开关。请确认：

1. Codex 设置 → **Computer Use** 已安装插件（`computer-use@openai-bundled` enabled）
2. 托盘 → **重新同步配置**，然后**完全退出并重启 Codex Desktop**
3. **新开一条对话**再试（旧对话可能在 `js_repl=false` 时创建，线程里不会注入 `mcp__node_repl__js`）
4. 使用 `$computer-use` 或 `@Computer Use`，例如：`打开 QQ 音乐播放七里香`

若重启后仍失败，打开 `%USERPROFILE%\.codex\config.toml` 检查是否又被写回 `js_repl = false`（Codex 已知 bug，见 [openai/codex#25090](https://github.com/openai/codex/issues/25090)）。可再次点「重新同步配置」，或手动改为 `js_repl = true` 后重启。

若配置都正常但新对话仍报 Node REPL 不可用，多半是 Codex Desktop 的线程级 bug（[openai/codex#21301](https://github.com/openai/codex/issues/21301)）：同一会话里 `mcp__node_repl__js` 可能未注入。可执行 `codex-helper doctor` 检查；或临时切回 OpenAI 官方模型验证 Computer Use 是否正常。

---

## 许可证

MIT © 2026

---

## 免责声明

本项目为非官方工具，与 OpenAI、DeepSeek 等公司无关联。请遵守各模型服务商使用条款；API Key 仅存于本地，请妥善保管。
