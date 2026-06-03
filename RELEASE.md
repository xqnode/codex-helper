# 版本发布说明

## 当前版本

**v0.1.0**（2026-06-03）

## 下载

预编译 Windows 安装包见 [GitHub Releases](https://github.com/xqnode/codex-helper/releases)：

| 文件 | 说明 |
|------|------|
| `CodexHelper-0.1.0-win64.zip` | 便携版 — 解压后运行 `codex-helper.exe` |
| `CodexHelper-0.1.0-Setup.exe` | Inno Setup 安装包（中文界面） |

### 运行要求

- Windows 10/11（64 位）
- [WebView2 运行时](https://developer.microsoft.com/microsoft-edge/webview2/)（Windows 11 通常已预装）
- [Codex CLI](https://github.com/openai/codex) 或 Codex Desktop
- 至少一个支持厂商的 API Key

## 从源码构建

```powershell
# 编译 Release + 打包 ZIP
.\build-zip.bat

# 编译 Release + ZIP + 安装包（需 Inno Setup 6）
.\build-all.bat
```

产物输出到 `dist/` 目录。

## 发布新版本

1. 修改 `Cargo.toml` 中的 `version`，并更新 `CHANGELOG.md`。
2. 构建产物：运行 `.\build-all.bat`
3. 提交并打标签：

   ```powershell
   git tag v0.1.1
   git push origin main --tags
   ```

4. 创建 GitHub Release，上传 `dist/*.zip` 与 `dist/*Setup.exe`：

   ```powershell
   gh release create v0.1.1 dist/CodexHelper-0.1.1-win64.zip dist/CodexHelper-0.1.1-Setup.exe `
     --title "v0.1.1" `
     --notes-file CHANGELOG.md
   ```

## 版本规则

本项目遵循[语义化版本](https://semver.org/lang/zh-CN/)：

- **主版本号** — 不兼容的配置或代理行为变更
- **次版本号** — 新增厂商、模型或功能
- **修订号** — Bug 修复与小改进
