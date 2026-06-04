//! Codex Responses ↔ Chat Completions 兼容层
//!
//! 本模块大部分代码来自上游开源项目 cc-switch（MIT 协议）：
//!   https://github.com/farion1231/cc-switch
//!   src-tauri/src/proxy/{sse.rs, json_canonical.rs}
//!   src-tauri/src/proxy/providers/{codex_chat_common.rs,
//!                                  streaming_codex_chat.rs,
//!                                  transform_codex_chat.rs}
//!
//! 我们做的最小适配：
//! 1. 移除了 cc-switch 业务相关的依赖（ProxyState、ProxyError、Database 等）。
//! 2. 用一个轻量的 `CodexToolContext` 占位，把 namespace/custom tool 还原能力留作
//!    "永远走 plain function" 分支（DeepSeek/通义等上游 chat completions 也只
//!    支持普通 function calling，已足够）。
//! 3. 把 `json_canonical` 削减为只剩 streaming 实际需要的三个纯函数（去掉 sha2）。
//!
//! `streaming_codex_chat::create_responses_sse_stream_from_chat` 是入口：
//! 输入上游 chat completions SSE 字节流，输出 Codex Desktop 期望的 Responses
//! SSE 事件流（带 `event: response.created` 等命名事件）。

mod codex_chat_common;
mod codex_chat_helpers;
mod json_canonical;
mod sse;
mod streaming_codex_chat;

#[allow(unused_imports)]
pub use streaming_codex_chat::create_responses_sse_stream_from_chat;
pub(crate) use codex_chat_common::{
    append_reasoning_content, extract_reasoning_field_text, extract_reasoning_summary_text,
};
