# 更新日志

本项目的所有重要变更均记录在此文件中。

格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [0.1.0] - 2026-06-03

### 新增

- Windows 系统托盘应用，将 Codex CLI 代理到国产 OpenAI 兼容大模型 API
- 支持 DeepSeek、通义千问、智谱 GLM、Kimi（Moonshot）、MiniMax
- WebView2 设置界面：管理 API Key、切换厂商与模型
- 本地 HTTP 代理，支持热重载（`codex-helper reload`）
- 自动同步 `~/.codex/config.toml` 与模型目录
- CLI 命令：`init`、`start`、`doctor`、`settings`
- 便携 ZIP 与 Inno Setup 安装包构建脚本
- 29 项单元/集成测试

### 修复

- 关闭设置窗口不再导致整个托盘程序退出
- 模型目录仅展示当前激活厂商的模型
- 移除 Codex 配置中的 sandbox/approval 项，避免出现「自定义 (config.toml)」权限模式

[0.1.0]: https://github.com/xqnode/codex-helper/releases/tag/v0.1.0
