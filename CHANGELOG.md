# 更新日志

本项目的所有重要变更均记录在此文件中。

格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [0.1.0] - 2026-06-04

### 新增

- Windows 系统托盘应用，将 Codex CLI 代理到国产 OpenAI 兼容大模型 API
- 支持 DeepSeek、通义千问、智谱 GLM、Kimi（Moonshot）、MiniMax、小米 MiMo
- **中转站**自定义 Base URL + API Key，直连上游（不走系统代理）
- WebView2 设置界面：厂商 Chip 切换、API Key、Base URL（官方只读 / 中转站可编辑）
- 托盘 **三级模型菜单**：切换模型 → 供应商 → 具体型号
- **请求日志**窗口：Provider、模型、Token、耗时、费用估算，托盘可打开
- 本地 HTTP 代理，支持热重载（`codex-helper reload`）
- 自动同步 `~/.codex/config.toml` 与模型目录
- CLI 命令：`init`、`start`、`doctor`、`settings`
- 便携 ZIP 与 Inno Setup 安装包构建脚本
- 41 项单元/集成测试

### 修复

- 关闭设置窗口不再导致整个托盘程序退出
- 模型目录仅展示当前激活厂商的模型；内置 Base URL / 模型 ID 与官网对齐，默认旗舰模型
- 移除 Codex 配置中的 sandbox/approval 项，避免出现「自定义 (config.toml)」权限模式
- 中转站：压平 Codex 结构化 content、不转发客户端请求头，修复 `Unsupported content type`
- DeepSeek 思考模式：回传 `reasoning_content`，避免 thinking + tool 调用报错
- 多工具调用：合并连续 `function_call`，支持 `custom_tool_call` / MCP，修复 insufficient tool messages
- 千问切换后工具调用失效（转发 tools 到上游 API）
- rollout 会话文件缺失导致无法对话

[0.1.0]: https://github.com/xqnode/codex-helper/releases/tag/v0.1.0
