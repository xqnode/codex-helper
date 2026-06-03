# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-06-03

### Added

- Windows system tray app that proxies Codex CLI to OpenAI-compatible Chinese LLM APIs
- Supported providers: DeepSeek, Qwen, Zhipu (GLM), Kimi (Moonshot), MiniMax
- WebView2 settings UI for API keys, provider switching, and model selection
- Local HTTP proxy with hot reload (`codex-helper reload`)
- Automatic sync to `~/.codex/config.toml` and model catalog
- CLI commands: `init`, `start`, `doctor`, `settings`
- Portable ZIP and Inno Setup installer build scripts
- 29 unit/integration tests

### Fixed

- Closing the settings window no longer exits the entire tray application
- Model catalog lists only models from the active provider
- Removed sandbox/approval keys from Codex config to avoid unwanted "custom config" mode

[0.1.0]: https://github.com/xqnode/codex-helper/releases/tag/v0.1.0
