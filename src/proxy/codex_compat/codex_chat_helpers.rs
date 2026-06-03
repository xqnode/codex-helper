//! Trimmed subset of cc-switch (MIT, https://github.com/farion1231/cc-switch).
//! Source: src-tauri/src/proxy/providers/transform_codex_chat.rs
//!
//! We only need the helpers reachable from `streaming_codex_chat.rs`. The full
//! `CodexToolContext` upstream supports namespace/custom/tool_search routing
//! that cc-switch builds from the Responses-API request body; for our
//! pure-passthrough proxy we leave that as a no-op (every chat tool name is
//! treated as a plain function call), which is exactly what DeepSeek / Qwen /
//! GLM upstreams report back.

use serde_json::{json, Value};

use super::codex_chat_common::response_function_call_item;

/// A stripped-down stand-in for cc-switch's `CodexToolContext`.
///
/// `streaming_codex_chat.rs` uses the context to (a) ask whether a chat-tool
/// name corresponds to a Codex `custom_tool` (so the SSE converter can emit
/// `custom_tool_call`/`response.custom_tool_call_input_*` events instead of
/// `function_call_arguments_*`), and (b) look up namespace metadata when
/// rebuilding the final output items.
///
/// Without the upstream Responses-API request body in hand, we can't restore
/// any of that metadata; treating every tool as a vanilla function call is
/// safe because Codex Desktop accepts the function-call envelope for ordinary
/// chat-format upstreams.
#[derive(Debug, Clone, Default)]
pub(crate) struct CodexToolContext;

impl CodexToolContext {
    pub(crate) fn is_custom_tool_chat_name(&self, _chat_name: &str) -> bool {
        false
    }
}

pub(crate) fn response_tool_call_item_id_from_chat_name(
    call_id: &str,
    _chat_name: &str,
    _tool_context: &CodexToolContext,
) -> String {
    format!("fc_{call_id}")
}

pub(crate) fn response_tool_call_item_from_chat_name(
    item_id: &str,
    status: &str,
    call_id: &str,
    chat_name: &str,
    arguments: &str,
    reasoning: Option<&str>,
    _tool_context: &CodexToolContext,
) -> Value {
    response_function_call_item(item_id, status, call_id, chat_name, arguments, reasoning)
}

pub(crate) fn custom_tool_input_from_chat_arguments(arguments: &str) -> String {
    if arguments.trim().is_empty() {
        return String::new();
    }
    arguments.to_string()
}

pub(crate) fn chat_usage_to_responses_usage(usage: Option<&Value>) -> Value {
    let Some(usage) = usage.filter(|value| value.is_object() && !value.is_null()) else {
        return json!({
            "input_tokens": 0,
            "output_tokens": 0,
            "total_tokens": 0,
            "output_tokens_details": { "reasoning_tokens": 0 }
        });
    };

    let input_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total_tokens = usage
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(input_tokens + output_tokens);

    let mut result = json!({
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "total_tokens": total_tokens
    });

    if let Some(cached) = usage
        .pointer("/prompt_tokens_details/cached_tokens")
        .or_else(|| usage.pointer("/input_tokens_details/cached_tokens"))
        .and_then(|v| v.as_u64())
    {
        result["input_tokens_details"] = json!({ "cached_tokens": cached });
    }

    if let Some(details) = usage
        .get("completion_tokens_details")
        .filter(|v| v.is_object())
    {
        let mut details = details.clone();
        if details.get("reasoning_tokens").is_none() {
            details["reasoning_tokens"] = json!(0);
        }
        result["output_tokens_details"] = details;
    } else {
        result["output_tokens_details"] = json!({ "reasoning_tokens": 0 });
    }

    if let Some(cache_read) = usage.get("cache_read_input_tokens") {
        result["cache_read_input_tokens"] = cache_read.clone();
    }
    if let Some(cache_creation) = usage.get("cache_creation_input_tokens") {
        result["cache_creation_input_tokens"] = cache_creation.clone();
    }

    result
}

pub(crate) fn response_id_from_chat_id(id: Option<&str>) -> String {
    let id = id.unwrap_or("ccswitch");
    if id.starts_with("resp_") {
        id.to_string()
    } else {
        format!("resp_{id}")
    }
}

pub(crate) fn response_status_from_finish_reason(finish_reason: Option<&str>) -> &'static str {
    match finish_reason {
        Some("length") => "incomplete",
        _ => "completed",
    }
}
