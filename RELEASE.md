# 版本发布说明

## 当前版本

**v0.2.2**（2026-06-06）

---

## Agent / 维护者：Windows 一键发版（推荐）

> 给 Cursor Agent 或维护者用的**固定流程**。按顺序执行即可，无需猜 `gh` 在哪。

### 前置条件

| 项 | 说明 |
|----|------|
| Rust | `cargo build --release` 可用 |
| Git | 已配置 `origin`，`git push` 能成功 |
| GitHub 鉴权 | 本机 `git credential` 存有 token（曾成功 `git push` 即可） |
| GitHub CLI | 见下节「gh 路径」 |

### gh 路径（重要）

本机 **`gh` 通常不在 PATH**，而在：

```text
%TEMP%\gh-cli\bin\gh.exe
```

Agent 查找顺序：

1. `Get-Command gh`
2. `$env:TEMP\gh-cli\bin\gh.exe`

若不存在，下载便携版（仅需一次）：

```powershell
$ghDir = Join-Path $env:TEMP 'gh-cli'
$zip = Join-Path $env:TEMP 'gh.zip'
Invoke-WebRequest -Uri 'https://github.com/cli/cli/releases/download/v2.74.0/gh_2.74.0_windows_amd64.zip' -OutFile $zip
Expand-Archive $zip (Join-Path $ghDir 'extract') -Force
Copy-Item (Join-Path $ghDir 'extract\bin\gh.exe') (Join-Path $ghDir 'bin\gh.exe') -Force
```

鉴权 token（每次发版前设置）：

```powershell
$gh = "$env:TEMP\gh-cli\bin\gh.exe"
$env:GH_TOKEN = ("protocol=https`nhost=github.com`n" | git credential fill | Select-String '^password=').ToString().Split('=',2)[1]
```

### 一键脚本（首选）

在项目根目录 PowerShell 执行：

```powershell
# 常规发版（工作区已 commit）
.\scripts\release-windows.ps1

# 同版本重新发版（覆盖 zip + 强制更新 tag，如此次 v0.2.0 热修复）
.\scripts\release-windows.ps1 -Retag

# 跳过构建（dist 已有 zip）
.\scripts\release-windows.ps1 -Retag -SkipBuild
```

脚本会自动：

1. 结束正在运行的 `codex-helper.exe`（避免 exe 被占用）
2. `cargo build --release` + 打 ZIP → `dist/CodexHelper-{version}-win64.zip`
3. `git tag v{version}` 并 `git push origin main` + push tag
4. `gh release upload`（已存在则 `--clobber`）+ 用 `dist/RELEASE_NOTES_v{version}.md` 更新说明

### Agent 手动逐步（脚本失败时）

**0. 发版前改文档**

- `Cargo.toml` → `version = "x.y.z"`（新版本时）
- `CHANGELOG.md` → 新增 `[x.y.z]` 条目
- `dist/RELEASE_NOTES_vX.Y.Z.md` → Release 页展示文案（**必须有**，脚本读这个）
- `RELEASE.md` → 更新「当前版本」一行

**1. 构建**（产出 **ZIP + Setup.exe**，与 GitHub Release 一致）

```powershell
taskkill /F /IM codex-helper.exe 2>$null
.\scripts\build-all.bat          # ZIP + CodexHelper-x.y.z-Setup.exe（需 Inno Setup 6）
# 仅便携 zip：.\scripts\build-zip.bat
```

首次构建 Setup 需 [Inno Setup 6](https://jrsoftware.org/isdl.php)。静默安装：

```powershell
Invoke-WebRequest -Uri "https://jrsoftware.org/download.php/is.exe?site=1" -OutFile "$env:TEMP\innosetup.exe"
Start-Process "$env:TEMP\innosetup.exe" -ArgumentList "/VERYSILENT","/SP-" -Wait
```

脚本会自动下载中文语言包 `ChineseSimplified.isl`（Inno 默认不带）。

**2. 提交 & 打标签**

```powershell
git add CHANGELOG.md RELEASE.md README.md src/ ...
git commit -m "fix: ..."   # 或 chore: release vX.Y.Z

# 新版本
git tag vX.Y.Z
git push origin main
git push origin vX.Y.Z

# 同版本热修复（覆盖 tag）
git tag -d vX.Y.Z
git tag vX.Y.Z
git push origin main
git push origin vX.Y.Z --force
```

**3. 上传 GitHub Release**

```powershell
$gh = "$env:TEMP\gh-cli\bin\gh.exe"
$env:GH_TOKEN = ("protocol=https`nhost=github.com`n" | git credential fill | Select-String '^password=').ToString().Split('=',2)[1]
$ver = "v0.2.0"   # 与 Cargo.toml 一致，带 v 前缀

# 首次创建
& $gh release create $ver "dist\CodexHelper-0.2.0-win64.zip" `
  --repo xqnode/codex-helper --title $ver --notes-file "dist\RELEASE_NOTES_$ver.md"

# 已存在 → 覆盖 zip + Setup.exe + 更新说明
& $gh release upload $ver "dist\CodexHelper-0.2.0-win64.zip" "dist\CodexHelper-0.2.0-Setup.exe" --repo xqnode/codex-helper --clobber
& $gh release edit $ver --repo xqnode/codex-helper --notes-file "dist\RELEASE_NOTES_$ver.md"

# 验证
& $gh release view $ver --repo xqnode/codex-helper
```

**4. 可选：Setup 安装包**

Inno Setup 构建后单独上传：

```powershell
& $gh release upload $ver "dist\CodexHelper-0.2.0-Setup.exe" --repo xqnode/codex-helper --clobber
```

### 发版检查清单

- [ ] `cargo test` 通过
- [ ] `dist/CodexHelper-{version}-win64.zip` 大小合理（约 2–4 MB）
- [ ] `git tag v{version}` 指向最新 commit
- [ ] https://github.com/xqnode/codex-helper/releases/tag/v{version} 可下载且说明正确

---

## 下载

预编译安装包见 [GitHub Releases](https://github.com/xqnode/codex-helper/releases)：

| 文件 | 说明 |
|------|------|
| `CodexHelper-0.2.0-win64.zip` | Windows 便携版 — 解压后运行 `codex-helper.exe` |
| `CodexHelper-0.2.0-Setup.exe` | Windows Inno Setup 安装包（中文界面，需本地 Inno Setup 构建） |
| `CodexHelper-0.1.0-macos.dmg` | macOS — 见 [v0.1.0 Release](https://github.com/xqnode/codex-helper/releases/tag/v0.1.0)（macOS 需在 Mac 上自行构建） |

### 运行要求

**Windows**

- Windows 10/11（64 位）
- [WebView2 运行时](https://developer.microsoft.com/microsoft-edge/webview2/)（Windows 11 通常已预装）

**macOS**

- macOS 12+（Apple Silicon / Intel）
- 测试包未签名/未公证：若提示「已损坏」，终端执行 `sudo xattr -cr "/Applications/Codex Helper.app"` 后重开；尽量从 GitHub 直接下载，勿经微信传输

**通用**

- [Codex CLI](https://github.com/openai/codex) 或 Codex Desktop
- 至少一个支持厂商的 API Key

---

## 从源码构建

```powershell
# 仅 ZIP（便携版）
.\scripts\build-zip.bat

# ZIP + Inno Setup 安装包
.\scripts\build-all.bat
```

产物输出到 `dist/`（已在 `.gitignore`，不提交仓库）。

---

## macOS 发版（简述）

在 Mac 上执行：

```bash
./scripts/build-macos-release.sh
# 产物在 dist/，手动 gh release upload
```

---

## 版本规则

本项目遵循[语义化版本](https://semver.org/lang/zh-CN/)：

- **主版本号** — 不兼容的配置或代理行为变更
- **次版本号** — 新增厂商、模型或功能
- **修订号** — Bug 修复与小改进
