# 更新日志

本项目的所有重要变更均记录在此文件中。

格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [Unreleased]

## [0.2.0] - 2026-06-06

### 新增

- **Computer Use 一键修复**：托盘「修复 Computer Use（桌面控制）」与 `codex-helper repair-computer-use`，绕过 Codex Desktop 对 `openai-bundled` 的过滤
- **厂商感知推理参数映射**（`codex_chat_reasoning` / `reasoning_options`）：DeepSeek、MiniMax、智谱、Kimi、千问、MiMo、OpenRouter 等按各自 API 形态注入 `thinking` / `reasoning_effort` / `enable_thinking`
- **设置页推理档位**：可配置默认 `model_reasoning_effort`（默认 `medium`），写入 Codex `config.toml`
- **内置工具历史合成**：`web_search` / `file_search` / `local_shell` 等 Responses 内置调用转为标准 assistant+tool 历史，避免上游格式拒绝
- **上游有限重试**：429 / 502 / 503 / 504 及连接/超时错误，最多 3 次，指数退避（500ms→1s→2s），尊重 `Retry-After`（封顶 30s）
- **Responses 上游错误包装**：非 2xx 转为 Responses `failed` JSON/SSE，客户端不再收到裸 Chat 错误
- **可选 tool 输出截断**：`tool_output_max_chars`（默认 `0` 关闭），开启后对超长 `role: tool` 文本做 head+tail 截断
- **Chat↔Responses 双向转换增强**：`chat_to_responses`、`codex_tool_context`、流式 SSE 还原 namespace / custom / tool_search
- **附件占位**：`input_file` / 音视频等不支持类型转为文本占位，避免上游 multimodal 报错
- 单元测试由 41 项增至 **109 项**

### 变更

- 流式读空闲超时 **300s**、非流式总超时 **600s**、连接超时 30s；流式客户端无总超时，适配思考模型慢首 token
- `reasoning_content` 优先回传真实推理（summary / 前序 assistant），仅兜底时使用 `"tool call"` 占位
- Responses 转发路径去掉双重 `repair`，仅 `patch_upstream_model`，减少历史被重复改写
- 内置工具 arguments/result 分离；`local_shell_call_output` 只传 output，降低 input token
- 本地代理默认端口固定 **25543**（自 v0.1.0 后续文档对齐）

### 修复

- Codex Desktop 插件页安装 Computer Use 失败（「插件安装失败」）；`doctor` 增加 Computer Use 安装状态检测
- 多轮 tool 历史：`repair_messages_for_upstream` 合并连续 assistant tool_calls、补齐缺失 tool 回复、system 合并至首条
- DeepSeek / Kimi 等 thinking 模型多轮 tool 时 `reasoning_content` 缺失导致 400
- MiniMax / 各厂商 reasoning 参数形态审计与修正
- Codex `insufficient tool messages` 类错误的多轮会话修复
- 流式 SSE 多字节字符边界、超大 remainder 防御性 flush

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

- 同步 Codex 配置时强制 `[features] js_repl = true`，修复 Computer Use / Browser Use 报「Node REPL 工具不可用」
- 关闭设置窗口不再导致整个托盘程序退出
- 模型目录仅展示当前激活厂商的模型；内置 Base URL / 模型 ID 与官网对齐，默认旗舰模型
- 移除 Codex 配置中的 sandbox/approval 项，避免出现「自定义 (config.toml)」权限模式
- 中转站：压平 Codex 结构化 content、不转发客户端请求头，修复 `Unsupported content type`
- DeepSeek 思考模式：回传 `reasoning_content`，避免 thinking + tool 调用报错
- 多工具调用：合并连续 `function_call`，支持 `custom_tool_call` / MCP，修复 insufficient tool messages
- 千问切换后工具调用失效（转发 tools 到上游 API）
- rollout 会话文件缺失导致无法对话

[0.2.0]: https://github.com/xqnode/codex-helper/releases/tag/v0.2.0
[0.1.0]: https://github.com/xqnode/codex-helper/releases/tag/v0.1.0
