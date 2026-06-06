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
//! 2. 完整 `CodexToolContext` 见 `proxy::codex_tool_context`，支持 namespace /
//!    custom / tool_search 双向还原。
//! 3. `json_canonical` 保留 canonical 序列化与 namespace 工具名哈希截断。
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
    append_reasoning_content, attach_optional_reasoning_content_field,
    extract_reasoning_field_text, extract_reasoning_summary_text, response_function_call_item,
    response_function_call_item_with_namespace, split_leading_think_block,
};
pub(crate) use codex_chat_helpers::{
    chat_usage_to_responses_usage, response_id_from_chat_id, response_status_from_finish_reason,
    response_tool_call_item_from_chat_name, response_tool_call_item_id_from_chat_name,
    CodexToolContext,
};
pub(crate) use json_canonical::{
    canonical_json_string, canonicalize_json_string_if_parseable, canonicalize_tool_arguments,
    short_sha256_hex,
};
pub use streaming_codex_chat::create_responses_sse_stream_from_chat_with_context;
