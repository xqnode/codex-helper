//! Responses API 请求 → Chat Completions 请求（保留 tools / 多轮 tool 历史）。

use std::collections::HashMap;

use serde_json::{json, Value};

use tracing::debug;

use crate::config::ProviderConfig;

use super::codex_compat::{
    append_reasoning_content, canonical_json_string, canonicalize_json_string_if_parseable,
    extract_reasoning_field_text, extract_reasoning_summary_text,
};
use super::codex_tool_context::{
    build_codex_tool_context_from_request, responses_custom_tool_call_to_chat_tool_call,
    responses_function_call_to_chat_tool_call, responses_tool_choice_to_chat,
    responses_tool_search_call_to_chat_tool_call, CodexToolContext,
};

const REASONING_PLACEHOLDER: &str = "tool call";

const EXTRA_CHAT_PASSTHROUGH_FIELDS: &[&str] = &[
    "frequency_penalty",
    "logit_bias",
    "logprobs",
    "metadata",
    "n",
    "parallel_tool_calls",
    "presence_penalty",
    "response_format",
    "seed",
    "service_tier",
    "stop",
    "stream_options",
    "top_logprobs",
    "user",
];

#[derive(Debug, Clone)]
pub struct ConvertedChatRequest {
    pub body: Vec<u8>,
    pub model: String,
    pub stream: bool,
    pub tool_context: CodexToolContext,
}

pub fn convert_responses_to_chat(body: &axum::body::Bytes) -> anyhow::Result<ConvertedChatRequest> {
    convert_responses_to_chat_with_provider(body, None, 0)
}

pub fn convert_responses_to_chat_with_provider(
    body: &axum::body::Bytes,
    provider: Option<&ProviderConfig>,
    tool_output_max_chars: usize,
) -> anyhow::Result<ConvertedChatRequest> {
    let value: Value = serde_json::from_slice(body)?;
    let tool_context = build_codex_tool_context_from_request(&value);
    let model = value
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("deepseek-v4-flash")
        .to_string();
    let stream = value.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);

    let mut messages = if let Some(existing) = value.get("messages").and_then(|m| m.as_array()) {
        if existing.is_empty() {
            Vec::new()
        } else {
            existing.clone()
        }
    } else {
        extract_messages_from_responses(&value, &tool_context)?
    };

    if messages.is_empty() {
        anyhow::bail!("无法从 Responses 请求中提取 input/instructions");
    }

    normalize_messages_for_upstream(&mut messages);
    repair_messages_for_upstream_with_options(
        &mut messages,
        repair_options_for_provider(provider, tool_output_max_chars),
    );

    let mut chat = json!({
        "model": model,
        "messages": messages,
        "stream": stream,
    });

    let tools = tool_context.chat_tools();
    if !tools.is_empty() {
        chat["tools"] = json!(tools);
    }
    if let Some(tool_choice) = value.get("tool_choice") {
        chat["tool_choice"] = responses_tool_choice_to_chat(tool_choice, &tool_context);
    }
    apply_max_tokens_fields(&mut chat, &value, &model);
    for key in ["temperature", "top_p"] {
        if let Some(v) = value.get(key) {
            chat[key] = v.clone();
        }
    }
    if let Some(reasoning) = value.get("reasoning") {
        chat["reasoning"] = reasoning.clone();
    }
    for key in EXTRA_CHAT_PASSTHROUGH_FIELDS {
        if let Some(v) = value.get(*key) {
            chat[*key] = v.clone();
        }
    }

    finalize_chat_request(&mut chat, stream);
    if let Some(provider) = provider {
        super::reasoning_options::apply_reasoning_options(&mut chat, provider);
    }
    Ok(ConvertedChatRequest {
        body: serde_json::to_vec(&chat)?,
        model,
        stream,
        tool_context,
    })
}

fn extract_messages_from_responses(
    value: &Value,
    tool_context: &CodexToolContext,
) -> anyhow::Result<Vec<Value>> {
    let mut messages = Vec::new();

    if let Some(instructions) = value.get("instructions") {
        let instructions = instruction_text(instructions);
        if !instructions.trim().is_empty() {
            messages.push(json!({
                "role": "system",
                "content": instructions,
            }));
        }
    }

    if let Some(input) = value.get("input") {
        append_responses_input_as_chat_messages(input, &mut messages, tool_context)?;
    }

    Ok(messages)
}

fn append_responses_input_as_chat_messages(
    input: &Value,
    messages: &mut Vec<Value>,
    tool_context: &CodexToolContext,
) -> anyhow::Result<()> {
    let mut pending_tool_calls = Vec::new();
    let mut pending_reasoning: Option<String> = None;
    let mut last_assistant_index: Option<usize> = None;

    match input {
        Value::String(text) => {
            messages.push(json!({
                "role": "user",
                "content": text,
            }));
        }
        Value::Array(items) => {
            for item in items {
                append_responses_item_as_chat_message(
                    item,
                    messages,
                    &mut pending_tool_calls,
                    &mut pending_reasoning,
                    &mut last_assistant_index,
                    tool_context,
                )?;
            }
        }
        Value::Object(_) => {
            append_responses_item_as_chat_message(
                input,
                messages,
                &mut pending_tool_calls,
                &mut pending_reasoning,
                &mut last_assistant_index,
                tool_context,
            )?;
        }
        _ => {}
    }

    flush_pending_tool_calls(
        messages,
        &mut pending_tool_calls,
        &mut pending_reasoning,
        &mut last_assistant_index,
    );
    Ok(())
}

fn append_responses_item_as_chat_message(
    item: &Value,
    messages: &mut Vec<Value>,
    pending_tool_calls: &mut Vec<Value>,
    pending_reasoning: &mut Option<String>,
    last_assistant_index: &mut Option<usize>,
    tool_context: &CodexToolContext,
) -> anyhow::Result<()> {
    let item_type = item.get("type").and_then(|v| v.as_str());
    match item_type {
        Some("function_call") => {
            append_unique_pending_reasoning(pending_reasoning, responses_item_reasoning_text(item));
            pending_tool_calls.push(responses_function_call_to_chat_tool_call(item, tool_context));
        }
        Some("custom_tool_call") => {
            append_unique_pending_reasoning(pending_reasoning, responses_item_reasoning_text(item));
            pending_tool_calls.push(responses_custom_tool_call_to_chat_tool_call(item));
        }
        Some("tool_search_call") => {
            append_unique_pending_reasoning(pending_reasoning, responses_item_reasoning_text(item));
            pending_tool_calls.push(responses_tool_search_call_to_chat_tool_call(item));
        }
        Some("local_shell_call") => {
            append_unique_pending_reasoning(pending_reasoning, responses_item_reasoning_text(item));
            pending_tool_calls.push(responses_builtin_tool_call_to_chat(item, "local_shell_call"));
        }
        Some("web_search_call")
        | Some("file_search_call")
        | Some("code_interpreter_call")
        | Some("image_generation_call") => {
            append_unique_pending_reasoning(pending_reasoning, responses_item_reasoning_text(item));
            append_self_contained_builtin_tool_history(
                item,
                item_type.unwrap_or("builtin_tool"),
                messages,
                pending_tool_calls,
                pending_reasoning,
                last_assistant_index,
            );
        }
        Some("function_call_output") => {
            flush_pending_tool_calls(
                messages,
                pending_tool_calls,
                pending_reasoning,
                last_assistant_index,
            );
            let call_id = responses_output_call_id(item);
            let output = match item.get("output") {
                Some(Value::String(s)) => canonicalize_json_string_if_parseable(s),
                Some(v) => canonical_json_string(v),
                None => String::new(),
            };
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": output,
            }));
        }
        Some("local_shell_call_output") => {
            flush_pending_tool_calls(
                messages,
                pending_tool_calls,
                pending_reasoning,
                last_assistant_index,
            );
            let call_id = responses_output_call_id(item);
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": responses_tool_output_content(item),
            }));
        }
        Some("custom_tool_call_output") | Some("tool_search_output") => {
            flush_pending_tool_calls(
                messages,
                pending_tool_calls,
                pending_reasoning,
                last_assistant_index,
            );
            let call_id = responses_output_call_id(item);
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": canonical_json_string(item),
            }));
        }
        Some("reasoning") => {
            let reasoning = extract_reasoning_summary_text(item);
            let attached_to_previous = pending_tool_calls.is_empty()
                && attach_reasoning_to_last_assistant(messages, *last_assistant_index, &reasoning);
            if !attached_to_previous {
                append_pending_reasoning(pending_reasoning, reasoning);
            }
        }
        Some("message") | None => {
            flush_pending_tool_calls(
                messages,
                pending_tool_calls,
                pending_reasoning,
                last_assistant_index,
            );
            if item.get("role").is_some() || item.get("content").is_some() {
                let message = responses_message_item_to_chat_message(item, pending_reasoning);
                update_last_assistant_index(messages, &message, last_assistant_index);
                messages.push(message);
            }
        }
        Some("item_reference") => {
            flush_pending_tool_calls(
                messages,
                pending_tool_calls,
                pending_reasoning,
                last_assistant_index,
            );
            if let Some(reference) = item_reference_to_chat_message(item) {
                update_last_assistant_index(messages, &reference, last_assistant_index);
                messages.push(reference);
            }
        }
        _ => {
            flush_pending_tool_calls(
                messages,
                pending_tool_calls,
                pending_reasoning,
                last_assistant_index,
            );
            if item.get("role").is_some() || item.get("content").is_some() {
                let message = responses_message_item_to_chat_message(item, pending_reasoning);
                update_last_assistant_index(messages, &message, last_assistant_index);
                messages.push(message);
            }
        }
    }

    Ok(())
}

const CODEX_BUILTIN_TOOL_PREFIX: &str = "codex_builtin__";

/// Codex 内置工具（web/file search 等）在 Responses `input` 里常以「已完成调用」单条出现；
/// 上游 Chat API 不认这些 type，合成 assistant+tool 对保留上下文。
fn append_self_contained_builtin_tool_history(
    item: &Value,
    item_type: &str,
    messages: &mut Vec<Value>,
    pending_tool_calls: &mut Vec<Value>,
    pending_reasoning: &mut Option<String>,
    last_assistant_index: &mut Option<usize>,
) {
    flush_pending_tool_calls(
        messages,
        pending_tool_calls,
        pending_reasoning,
        last_assistant_index,
    );

    let call_id = builtin_tool_call_id(item);
    let mut assistant = json!({
        "role": "assistant",
        "content": null,
        "tool_calls": [responses_builtin_tool_call_to_chat_with_id(item, item_type, &call_id)],
    });
    attach_pending_reasoning_to_assistant(&mut assistant, pending_reasoning);
    *last_assistant_index = Some(messages.len());
    messages.push(assistant);
    messages.push(json!({
        "role": "tool",
        "tool_call_id": call_id,
        "content": builtin_tool_result_content(item, item_type),
    }));
}

fn responses_builtin_tool_call_to_chat(item: &Value, item_type: &str) -> Value {
    let call_id = builtin_tool_call_id(item);
    responses_builtin_tool_call_to_chat_with_id(item, item_type, &call_id)
}

fn responses_builtin_tool_call_to_chat_with_id(
    item: &Value,
    item_type: &str,
    call_id: &str,
) -> Value {
    let tool_name = builtin_tool_chat_name(item_type);
    json!({
        "id": call_id,
        "type": "function",
        "function": {
            "name": format!("{CODEX_BUILTIN_TOOL_PREFIX}{tool_name}"),
            "arguments": canonical_json_string(&builtin_tool_call_arguments(item, item_type)),
        }
    })
}

fn builtin_tool_chat_name(item_type: &str) -> &str {
    match item_type {
        "web_search_call" => "web_search",
        "file_search_call" => "file_search",
        "code_interpreter_call" => "code_interpreter",
        "image_generation_call" => "image_generation",
        "local_shell_call" => "local_shell",
        _ => "tool",
    }
}

fn builtin_tool_call_id(item: &Value) -> String {
    let call_id = responses_output_call_id(item);
    if !call_id.is_empty() {
        return call_id.to_string();
    }
    item.get("id")
        .and_then(|v| v.as_str())
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "codex_builtin_call".to_string())
}

fn responses_tool_output_content(item: &Value) -> String {
    match item.get("output") {
        Some(Value::String(text)) => canonicalize_json_string_if_parseable(text),
        Some(value) => canonical_json_string(value),
        None => String::new(),
    }
}

/// tool_call 只保留调用意图；结果放到后续 `tool` 消息，避免同一 payload 写两遍。
fn builtin_tool_call_arguments(item: &Value, item_type: &str) -> Value {
    match item_type {
        "web_search_call" => compact_web_search_arguments(item),
        "file_search_call" => compact_file_search_arguments(item),
        "code_interpreter_call" => compact_code_interpreter_arguments(item),
        "image_generation_call" => compact_image_generation_arguments(item),
        "local_shell_call" => compact_local_shell_arguments(item),
        _ => compact_generic_builtin_arguments(item),
    }
}

fn builtin_tool_result_content(item: &Value, item_type: &str) -> String {
    let value = match item_type {
        "web_search_call" => compact_web_search_result(item),
        "file_search_call" => compact_file_search_result(item),
        "code_interpreter_call" => compact_code_interpreter_result(item),
        "image_generation_call" => compact_image_generation_result(item),
        "local_shell_call" => Value::String(responses_tool_output_content(item)),
        _ => compact_generic_builtin_result(item),
    };
    canonical_json_string(&value)
}

fn compact_generic_builtin_arguments(item: &Value) -> Value {
    let Some(obj) = item.as_object() else {
        return item.clone();
    };
    let mut args = obj.clone();
    args.remove("type");
    args.remove("results");
    args.remove("output");
    args.remove("outputs");
    Value::Object(args)
}

fn compact_generic_builtin_result(item: &Value) -> Value {
    let Some(obj) = item.as_object() else {
        return item.clone();
    };
    let mut result = serde_json::Map::new();
    if let Some(status) = obj.get("status") {
        result.insert("status".into(), status.clone());
    }
    for key in ["results", "output", "outputs", "action"] {
        if let Some(value) = obj.get(key) {
            result.insert(key.into(), value.clone());
        }
    }
    Value::Object(result)
}

fn compact_web_search_arguments(item: &Value) -> Value {
    let mut args = serde_json::Map::new();
    if let Some(status) = item.get("status") {
        args.insert("status".into(), status.clone());
    }
    if let Some(action) = item.get("action") {
        args.insert("action".into(), compact_web_search_action(action));
    }
    Value::Object(args)
}

fn compact_web_search_action(action: &Value) -> Value {
    let Some(obj) = action.as_object() else {
        return action.clone();
    };
    let mut compact = obj.clone();
    compact.remove("sources");
    compact.remove("results");
    Value::Object(compact)
}

fn compact_web_search_result(item: &Value) -> Value {
    let mut result = serde_json::Map::new();
    if let Some(status) = item.get("status") {
        result.insert("status".into(), status.clone());
    }
    if let Some(results) = web_search_results_from_item(item) {
        result.insert("results".into(), compact_url_hits(results));
    }
    Value::Object(result)
}

fn web_search_results_from_item(item: &Value) -> Option<&Value> {
    item.get("results").or_else(|| {
        item.get("action")
            .and_then(|action| action.get("sources").or_else(|| action.get("results")))
    })
}

fn compact_file_search_arguments(item: &Value) -> Value {
    json!({
        "status": item.get("status").cloned().unwrap_or(Value::Null),
        "queries": item.get("queries").cloned().unwrap_or_else(|| json!([])),
    })
}

fn compact_file_search_result(item: &Value) -> Value {
    let results = item
        .get("results")
        .and_then(|value| value.as_array())
        .map(|hits| {
            hits.iter()
                .take(20)
                .map(compact_file_search_hit)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({
        "status": item.get("status").cloned().unwrap_or(Value::Null),
        "results": results,
    })
}

fn compact_file_search_hit(hit: &Value) -> Value {
    json!({
        "filename": hit.get("filename").or_else(|| hit.get("file_id")),
        "score": hit.get("score"),
    })
}

fn compact_code_interpreter_arguments(item: &Value) -> Value {
    json!({
        "status": item.get("status").cloned().unwrap_or(Value::Null),
        "container_id": item.get("container_id").cloned().unwrap_or(Value::Null),
    })
}

fn compact_code_interpreter_result(item: &Value) -> Value {
    json!({
        "status": item.get("status").cloned().unwrap_or(Value::Null),
        "outputs": item.get("outputs").or_else(|| item.get("output")).cloned().unwrap_or(Value::Null),
    })
}

fn compact_image_generation_arguments(item: &Value) -> Value {
    json!({
        "status": item.get("status").cloned().unwrap_or(Value::Null),
        "prompt": item.get("prompt")
            .or_else(|| item.get("revised_prompt"))
            .cloned()
            .unwrap_or(Value::Null),
    })
}

fn compact_image_generation_result(item: &Value) -> Value {
    json!({
        "status": item.get("status").cloned().unwrap_or(Value::Null),
        "result": item.get("result")
            .or_else(|| item.get("url"))
            .or_else(|| item.get("image_url"))
            .cloned()
            .unwrap_or(Value::Null),
    })
}

fn compact_local_shell_arguments(item: &Value) -> Value {
    json!({
        "status": item.get("status").cloned().unwrap_or(Value::Null),
        "action": item.get("action").cloned().unwrap_or(Value::Null),
    })
}

fn compact_url_hits(results: &Value) -> Value {
    let Some(hits) = results.as_array() else {
        return results.clone();
    };
    Value::Array(
        hits.iter()
            .take(20)
            .map(|hit| {
                json!({
                    "url": hit.get("url"),
                    "title": hit.get("title").or_else(|| hit.get("name")),
                })
            })
            .collect(),
    )
}

fn item_reference_to_chat_message(item: &Value) -> Option<Value> {
    let id = item.get("id").and_then(|v| v.as_str()).filter(|s| !s.is_empty())?;
    Some(json!({
        "role": "user",
        "content": format!("[Referenced prior item: {id}]"),
    }))
}

fn flush_pending_tool_calls(
    messages: &mut Vec<Value>,
    pending_tool_calls: &mut Vec<Value>,
    pending_reasoning: &mut Option<String>,
    last_assistant_index: &mut Option<usize>,
) {
    if pending_tool_calls.is_empty() {
        return;
    }

    let mut message = json!({
        "role": "assistant",
        "content": null,
        "tool_calls": std::mem::take(pending_tool_calls),
    });
    attach_pending_reasoning_to_assistant(&mut message, pending_reasoning);
    *last_assistant_index = Some(messages.len());
    messages.push(message);
}

fn responses_message_item_to_chat_message(
    item: &Value,
    pending_reasoning: &mut Option<String>,
) -> Value {
    let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
    let chat_role = map_role_for_upstream(role);
    let content = item
        .get("content")
        .map(responses_content_to_chat_content)
        .unwrap_or(Value::Null);

    let mut message = json!({
        "role": chat_role,
        "content": content,
    });

    if chat_role == "assistant" {
        append_pending_reasoning(pending_reasoning, responses_item_reasoning_text(item));
        attach_pending_reasoning_to_assistant(&mut message, pending_reasoning);
    } else {
        pending_reasoning.take();
    }

    message
}

fn responses_item_reasoning_text(item: &Value) -> Option<String> {
    extract_reasoning_field_text(item)
}

fn append_pending_reasoning(pending_reasoning: &mut Option<String>, reasoning: Option<String>) {
    let Some(reasoning) = reasoning else {
        return;
    };
    let reasoning = reasoning.trim();
    if reasoning.is_empty() {
        return;
    }

    match pending_reasoning {
        Some(existing) if !existing.is_empty() => {
            existing.push_str("\n\n");
            existing.push_str(reasoning);
        }
        _ => {
            *pending_reasoning = Some(reasoning.to_string());
        }
    }
}

fn append_unique_pending_reasoning(
    pending_reasoning: &mut Option<String>,
    reasoning: Option<String>,
) {
    let Some(reasoning) = reasoning else {
        return;
    };
    let reasoning = reasoning.trim();
    if reasoning.is_empty() {
        return;
    }

    match pending_reasoning {
        Some(existing) if existing.contains(reasoning) => {}
        Some(existing) if !existing.is_empty() => {
            existing.push_str("\n\n");
            existing.push_str(reasoning);
        }
        _ => {
            *pending_reasoning = Some(reasoning.to_string());
        }
    }
}

fn attach_pending_reasoning_to_assistant(
    message: &mut Value,
    pending_reasoning: &mut Option<String>,
) {
    if let Some(reasoning) = pending_reasoning.take() {
        if let Some(obj) = message.as_object_mut() {
            append_reasoning_content(obj, &reasoning);
        }
    }
}

fn attach_reasoning_to_last_assistant(
    messages: &mut [Value],
    last_assistant_index: Option<usize>,
    reasoning: &Option<String>,
) -> bool {
    let Some(reasoning) = reasoning
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return true;
    };
    let Some(index) = last_assistant_index else {
        return false;
    };
    let Some(message) = messages.get_mut(index) else {
        return false;
    };
    if message.get("role").and_then(|v| v.as_str()) != Some("assistant") {
        return false;
    }

    if let Some(obj) = message.as_object_mut() {
        append_reasoning_content(obj, reasoning);
        return true;
    }

    false
}

fn update_last_assistant_index(
    messages: &[Value],
    message: &Value,
    last_assistant_index: &mut Option<usize>,
) {
    match message.get("role").and_then(|v| v.as_str()) {
        Some("assistant") => {
            *last_assistant_index = Some(messages.len());
        }
        Some("tool") => {}
        _ => {
            *last_assistant_index = None;
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RepairMessagesOptions {
    /// 为 assistant tool_calls 补 `reasoning_content` 占位；仅 thinking 模型需要。
    pub preserve_reasoning_content: bool,
    /// 超长 `role: tool` 文本 head+tail 截断上限；0 = 关闭。
    pub tool_output_max_chars: usize,
}

impl Default for RepairMessagesOptions {
    fn default() -> Self {
        Self {
            preserve_reasoning_content: false,
            tool_output_max_chars: 0,
        }
    }
}

pub fn repair_options_for_provider(
    provider: Option<&ProviderConfig>,
    tool_output_max_chars: usize,
) -> RepairMessagesOptions {
    RepairMessagesOptions {
        preserve_reasoning_content: provider
            .map(provider_needs_reasoning_content)
            .unwrap_or(false),
        tool_output_max_chars,
    }
}

/// 厂商是否需要 tool-call 历史携带 `reasoning_content`（见 `CodexChatReasoningConfig`）。
pub fn provider_needs_reasoning_content(provider: &ProviderConfig) -> bool {
    crate::provider::codex_chat_reasoning::provider_needs_reasoning_content(provider)
}

/// 修复多轮 tool 历史，避免上游报 insufficient tool messages。
#[allow(dead_code)]
pub fn repair_messages_for_upstream(messages: &mut Vec<Value>) {
    repair_messages_for_upstream_with_options(messages, RepairMessagesOptions::default());
}

pub fn repair_messages_for_upstream_with_options(
    messages: &mut Vec<Value>,
    options: RepairMessagesOptions,
) {
    repair_chat_tool_message_sequence(messages);
    backfill_missing_tool_responses(messages);
    normalize_assistant_tool_call_content(messages);
    if options.preserve_reasoning_content {
        backfill_tool_call_reasoning_placeholders(messages);
    }
    if options.tool_output_max_chars > 0 {
        truncate_large_tool_outputs(messages, options.tool_output_max_chars);
    }
    let collapsed = collapse_system_messages_to_head(std::mem::take(messages));
    messages.extend(collapsed);
}

/// 仅截断 `role: tool` 的纯文本 content，保留 head+tail；不触碰 tool_calls arguments。
pub fn truncate_large_tool_outputs(messages: &mut [Value], max_chars: usize) {
    if max_chars == 0 {
        return;
    }
    for message in messages.iter_mut() {
        if message.get("role").and_then(|v| v.as_str()) != Some("tool") {
            continue;
        }
        let Some(obj) = message.as_object_mut() else {
            continue;
        };
        let Some(content) = obj.get("content").and_then(|v| v.as_str()) else {
            continue;
        };
        let truncated = truncate_text_head_tail(content, max_chars);
        if truncated.len() != content.len() || truncated != content {
            let original_chars = content.chars().count();
            let truncated_chars = truncated.chars().count();
            debug!(
                original_chars,
                truncated_chars,
                max_chars,
                "truncated upstream tool output"
            );
            obj.insert("content".into(), Value::String(truncated));
        }
    }
}

pub fn truncate_text_head_tail(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return text.to_string();
    }
    let total_chars = text.chars().count();
    if total_chars <= max_chars {
        return text.to_string();
    }

    const MARKER_RESERVE: usize = 48;
    let content_budget = max_chars.saturating_sub(MARKER_RESERVE).max(32);
    let head_chars = content_budget / 2;
    let tail_chars = content_budget - head_chars;
    let omitted = total_chars.saturating_sub(head_chars + tail_chars);
    let marker = format!("\n\n[... truncated {omitted} chars ...]\n\n");
    let head: String = text.chars().take(head_chars).collect();
    let tail: String = text.chars().skip(total_chars - tail_chars).collect();
    format!("{head}{marker}{tail}")
}

/// 严格 OpenAI 兼容上游在无 tools 时会拒绝 tool_choice / parallel_tool_calls；
/// 流式请求需显式 include_usage 才能在 SSE 末尾拿到 usage。
pub fn finalize_chat_request(chat: &mut Value, stream: bool) {
    let has_tools = chat
        .get("tools")
        .and_then(|v| v.as_array())
        .is_some_and(|tools| !tools.is_empty());
    if !has_tools {
        if let Some(obj) = chat.as_object_mut() {
            obj.remove("tool_choice");
            obj.remove("parallel_tool_calls");
        }
    }

    if stream {
        match chat.get_mut("stream_options") {
            Some(Value::Object(opts)) => {
                opts.insert("include_usage".into(), json!(true));
            }
            _ => {
                chat["stream_options"] = json!({ "include_usage": true });
            }
        }
    }
}

/// MiniMax 等厂商要求 system 只能出现在首条；合并中间 system/developer 消息。
fn collapse_system_messages_to_head(messages: Vec<Value>) -> Vec<Value> {
    let mut system_chunks: Vec<String> = Vec::new();
    let mut rest: Vec<Value> = Vec::with_capacity(messages.len());

    for msg in messages {
        if msg.get("role").and_then(|v| v.as_str()) == Some("system") {
            if let Some(text) = msg.get("content").and_then(|v| v.as_str()) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    system_chunks.push(text.to_string());
                }
                continue;
            }
        }
        rest.push(msg);
    }

    let mut out: Vec<Value> = Vec::with_capacity(rest.len() + 1);
    if !system_chunks.is_empty() {
        out.push(json!({
            "role": "system",
            "content": system_chunks.join("\n\n"),
        }));
    }
    out.extend(rest);
    out
}

fn normalize_assistant_tool_call_content(messages: &mut [Value]) {
    for message in messages.iter_mut() {
        if !assistant_has_tool_calls(message) {
            continue;
        }
        let Some(obj) = message.as_object_mut() else {
            continue;
        };
        let is_nullish = obj
            .get("content")
            .is_none_or(|value| value.is_null());
        if is_nullish {
            obj.insert("content".into(), Value::String(String::new()));
        }
    }
}

/// 合并连续的 assistant+tool_calls 消息。
fn repair_chat_tool_message_sequence(messages: &mut Vec<Value>) {
    let mut i = 0;
    while i < messages.len() {
        if !assistant_has_tool_calls(&messages[i]) {
            i += 1;
            continue;
        }

        let mut merged = tool_calls_from(&messages[i]);
        let mut j = i + 1;
        while j < messages.len() && assistant_has_tool_calls(&messages[j]) {
            merged.extend(tool_calls_from(&messages[j]));
            j += 1;
        }

        if j > i + 1 {
            if let Some(obj) = messages[i].as_object_mut() {
                obj.insert("tool_calls".into(), Value::Array(merged));
            }
            messages.drain(i + 1..j);
        }

        i += 1;
    }
}

fn assistant_has_tool_calls(message: &Value) -> bool {
    message.get("role").and_then(|v| v.as_str()) == Some("assistant")
        && message
            .get("tool_calls")
            .and_then(|v| v.as_array())
            .is_some_and(|calls| !calls.is_empty())
}

fn tool_calls_from(message: &Value) -> Vec<Value> {
    message
        .get("tool_calls")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

fn tool_call_ids_from(message: &Value) -> Vec<String> {
    tool_calls_from(message)
        .iter()
        .filter_map(|call| call.get("id").and_then(|v| v.as_str()))
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect()
}

fn responses_output_call_id(item: &Value) -> &str {
    item.get("call_id")
        .or_else(|| item.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

/// 为每个 assistant tool_calls 补齐缺失的 tool 回复（按 tool_call_id 顺序）。
fn backfill_missing_tool_responses(messages: &mut Vec<Value>) {
    let mut i = 0;
    while i < messages.len() {
        if !assistant_has_tool_calls(&messages[i]) {
            i += 1;
            continue;
        }

        let expected_ids = tool_call_ids_from(&messages[i]);
        if expected_ids.is_empty() {
            i += 1;
            continue;
        }

        let tool_start = i + 1;
        let mut tool_end = tool_start;
        while tool_end < messages.len()
            && messages[tool_end].get("role").and_then(|v| v.as_str()) == Some("tool")
        {
            tool_end += 1;
        }

        let existing: HashMap<String, Value> = messages[tool_start..tool_end]
            .iter()
            .filter_map(|msg| {
                let id = msg.get("tool_call_id").and_then(|v| v.as_str())?;
                if id.is_empty() {
                    return None;
                }
                Some((id.to_string(), msg.clone()))
            })
            .collect();

        let rebuilt: Vec<Value> = expected_ids
            .iter()
            .map(|id| {
                existing.get(id).cloned().unwrap_or_else(|| {
                    json!({
                        "role": "tool",
                        "tool_call_id": id,
                        "content": "",
                    })
                })
            })
            .collect();

        let needs_rebuild = rebuilt.len() != tool_end - tool_start
            || rebuilt
                .iter()
                .zip(&messages[tool_start..tool_end])
                .any(|(expected, actual)| expected != actual);
        if needs_rebuild {
            messages.splice(tool_start..tool_end, rebuilt);
        }

        i = tool_start + expected_ids.len();
    }
}

fn backfill_tool_call_reasoning_placeholders(messages: &mut [Value]) {
    for index in 0..messages.len() {
        if !assistant_has_tool_calls(&messages[index]) {
            continue;
        }
        if message_has_reasoning_content(&messages[index]) {
            continue;
        }
        let reasoning = find_reasoning_for_tool_call_message(messages, index)
            .unwrap_or_else(|| REASONING_PLACEHOLDER.to_string());
        if let Some(obj) = messages[index].as_object_mut() {
            obj.insert("reasoning_content".into(), Value::String(reasoning));
        }
    }
}

fn message_has_reasoning_content(message: &Value) -> bool {
    responses_item_reasoning_text(message)
        .is_some_and(|text| !text.trim().is_empty())
}

/// 为缺 `reasoning_content` 的 tool_calls 向前查找最近一条真实推理文本。
/// 遇到更早的 assistant+tool_calls 即停止，避免跨轮次继承错误推理。
fn find_reasoning_for_tool_call_message(messages: &[Value], index: usize) -> Option<String> {
    for i in (0..index).rev() {
        if assistant_has_tool_calls(&messages[i]) {
            break;
        }
        if let Some(text) = responses_item_reasoning_text(&messages[i]) {
            return Some(text);
        }
    }
    None
}

fn instruction_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(|v| v.as_str())
                    .or_else(|| part.as_str())
            })
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        other => other.as_str().unwrap_or_default().to_string(),
    }
}

fn apply_max_tokens_fields(chat: &mut Value, source: &Value, model: &str) {
    if let Some(max_tokens) = source.get("max_output_tokens") {
        if is_openai_o_series(model) {
            chat["max_completion_tokens"] = max_tokens.clone();
        } else {
            chat["max_tokens"] = max_tokens.clone();
        }
    }
    if let Some(max_tokens) = source.get("max_tokens") {
        chat["max_tokens"] = max_tokens.clone();
    }
    if let Some(max_tokens) = source.get("max_completion_tokens") {
        chat["max_completion_tokens"] = max_tokens.clone();
    }
}

fn is_openai_o_series(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    model.starts_with("o1")
        || model.starts_with("o3")
        || model.starts_with("o4")
        || model.starts_with("gpt-5")
}

/// Responses 结构化 content → Chat content；纯文本压平，含图片时保留多模态数组。
fn responses_content_to_chat_content(content: &Value) -> Value {
    if content.is_null() || content.is_string() {
        return content.clone();
    }

    let Some(parts) = content.as_array() else {
        return normalize_content_for_upstream(content);
    };

    let mut chat_parts = Vec::new();
    let mut has_non_text_part = false;

    for part in parts {
        let item_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match item_type {
            "input_text" | "output_text" | "text" => {
                if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        chat_parts.push(json!({
                            "type": "text",
                            "text": text
                        }));
                    }
                }
            }
            "refusal" => {
                if let Some(text) = part.get("refusal").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        chat_parts.push(json!({
                            "type": "text",
                            "text": text
                        }));
                    }
                }
            }
            "input_image" => {
                if let Some(image_url) = part.get("image_url") {
                    let image_url = if image_url.is_object() {
                        image_url.clone()
                    } else {
                        json!({ "url": image_url.as_str().unwrap_or_default() })
                    };
                    chat_parts.push(json!({
                        "type": "image_url",
                        "image_url": image_url
                    }));
                    has_non_text_part = true;
                }
            }
            "image_url" => {
                if let Some(image_url) = part.get("image_url") {
                    chat_parts.push(json!({
                        "type": "image_url",
                        "image_url": image_url.clone()
                    }));
                    has_non_text_part = true;
                }
            }
            "input_file" | "input_audio" | "input_video" => {
                if let Some(text) = unsupported_attachment_part_to_text(part, item_type) {
                    chat_parts.push(json!({
                        "type": "text",
                        "text": text
                    }));
                }
            }
            _ => {}
        }
    }

    if !has_non_text_part {
        return Value::String(
            chat_parts
                .iter()
                .filter_map(|part| part.get("text").and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }

    Value::Array(chat_parts)
}

pub fn map_role_for_upstream(role: &str) -> &str {
    match role {
        "developer" => "system",
        "latest_reminder" => "user",
        _ => role,
    }
}

pub fn normalize_message_content_for_upstream(role: &str, content: &Value) -> Value {
    if matches!(role, "user" | "assistant") {
        if let Value::Array(parts) = content {
            if parts
                .iter()
                .any(|part| part.get("type").and_then(|v| v.as_str()) == Some("image_url"))
            {
                return content.clone();
            }
        }
        let converted = responses_content_to_chat_content(content);
        if converted.is_array() {
            return converted;
        }
    }
    normalize_content_for_upstream(content)
}

/// 将 Codex / Responses 的结构化 content 压平为纯字符串，兼容只认 text 的中转站。
pub fn normalize_content_for_upstream(content: &Value) -> Value {
    match content {
        Value::String(text) => Value::String(text.clone()),
        Value::Null => Value::String(String::new()),
        Value::Object(obj) => Value::String(extract_text_from_content_part(obj).unwrap_or_default()),
        Value::Array(items) => {
            let parts: Vec<String> = items
                .iter()
                .filter_map(extract_text_from_content_item)
                .collect();
            Value::String(parts.join("\n"))
        }
        _ => Value::String(String::new()),
    }
}

fn extract_text_from_content_item(item: &Value) -> Option<String> {
    if let Some(text) = item.as_str() {
        return Some(text.to_string());
    }
    item.as_object()
        .and_then(|obj| extract_text_from_content_part(obj))
}

fn unsupported_attachment_part_to_text(part: &Value, item_type: &str) -> Option<String> {
    let label = match item_type {
        "input_file" => "file",
        "input_audio" => "audio",
        "input_video" => "video",
        _ => return None,
    };
    let filename = part
        .get("filename")
        .or_else(|| part.get("file_id"))
        .or_else(|| part.get("name"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mime = part
        .get("mime_type")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty());

    Some(match (filename, mime) {
        (Some(name), Some(mime)) => format!("[Attached {label}: {name} ({mime})]"),
        (Some(name), None) => format!("[Attached {label}: {name}]"),
        (None, Some(mime)) => format!("[Attached {label} ({mime})]"),
        (None, None) => format!("[Attached {label}]"),
    })
}

fn extract_text_from_content_part(obj: &serde_json::Map<String, Value>) -> Option<String> {
    let item_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match item_type {
        "input_text" | "output_text" | "text" | "summarized_text" => obj
            .get("text")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        "input_image" | "image" | "image_url" => None,
        "input_file" | "input_audio" | "input_video" => {
            unsupported_attachment_part_to_text(&Value::Object(obj.clone()), item_type)
        }
        _ => obj
            .get("text")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    }
}

pub fn normalize_messages_for_upstream(messages: &mut [Value]) {
    for message in messages.iter_mut() {
        let Some(obj) = message.as_object_mut() else {
            continue;
        };
        if let Some(role) = obj.get("role").and_then(|v| v.as_str()) {
            let mapped = map_role_for_upstream(role).to_string();
            obj.insert("role".into(), Value::String(mapped));
        }
        if let Some(content) = obj.get("content").cloned() {
            let role = obj
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("user");
            obj.insert(
                "content".into(),
                normalize_message_content_for_upstream(role, &content),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deepseek_provider() -> ProviderConfig {
        ProviderConfig::new(
            "deepseek",
            "DeepSeek",
            "https://api.deepseek.com/v1",
            "DEEPSEEK_API_KEY",
            "deepseek-v4-pro",
            "deepseek-v4-pro",
            "responses",
        )
    }

    fn parse_chat(body: &'static [u8]) -> Value {
        let converted = convert_responses_to_chat(&axum::body::Bytes::from_static(body)).unwrap();
        serde_json::from_slice(&converted.body).unwrap()
    }

    #[test]
    fn convert_with_provider_maps_reasoning_at_transform_stage() {
        let body = br#"{
            "model": "deepseek-v4-pro",
            "stream": false,
            "reasoning": {"effort": "high"},
            "input": [{"role": "user", "content": "hi"}]
        }"#;
        let converted = convert_responses_to_chat_with_provider(
            &axum::body::Bytes::from_static(body),
            Some(&deepseek_provider()),
            0,
        )
        .unwrap();
        let chat: Value = serde_json::from_slice(&converted.body).unwrap();
        assert_eq!(chat["reasoning_effort"], "high");
        assert_eq!(chat["thinking"]["type"], "enabled");
        assert!(chat.get("reasoning").is_none());
    }

    #[test]
    fn preserves_input_image_as_multimodal_content() {
        let body = br#"{
            "model": "gpt-5.5",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "what is this"},
                    {"type": "input_image", "image_url": "https://example.com/a.png"}
                ]
            }]
        }"#;
        let v = parse_chat(body);
        let content = &v["messages"][0]["content"];
        assert!(content.is_array());
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "image_url");
    }

    #[test]
    fn preserves_input_file_as_text_placeholder() {
        let body = br#"{
            "model": "gpt-5.5",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "summarize"},
                    {"type": "input_file", "filename": "README.md", "mime_type": "text/markdown"}
                ]
            }]
        }"#;
        let v = parse_chat(body);
        assert_eq!(
            v["messages"][0]["content"],
            "summarize\n[Attached file: README.md (text/markdown)]"
        );
    }

    #[test]
    fn parses_array_instructions_into_system_message() {
        let body = br#"{
            "model": "gpt-5.5",
            "instructions": [{"type": "input_text", "text": "Be helpful."}],
            "input": [{"role": "user", "content": "hi"}]
        }"#;
        let v = parse_chat(body);
        assert_eq!(v["messages"][0]["role"], "system");
        assert_eq!(v["messages"][0]["content"], "Be helpful.");
    }

    #[test]
    fn forwards_reasoning_and_seed_fields() {
        let body = br#"{
            "model": "deepseek-v4-pro",
            "stream": true,
            "seed": 42,
            "reasoning": {"effort": "high"},
            "input": [{"role": "user", "content": "hi"}]
        }"#;
        let v = parse_chat(body);
        assert_eq!(v["reasoning"]["effort"], "high");
        assert_eq!(v["seed"], 42);
    }

    #[test]
    fn forwards_tools_from_responses_request() {
        let body = br#"{
            "model": "qwen3.7-plus",
            "stream": true,
            "tools": [{
                "type": "function",
                "name": "read_file",
                "description": "Read a file",
                "parameters": {"type": "object", "properties": {"path": {"type": "string"}}}
            }],
            "input": [{"role": "user", "content": "hi"}]
        }"#;
        let v = parse_chat(body);
        assert_eq!(v["tools"][0]["function"]["name"], "read_file");
        assert_eq!(v["messages"][0]["role"], "user");
    }

    #[test]
    fn flattens_structured_user_content_to_plain_string() {
        let body = br#"{
            "model": "gpt-5.5",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": "hello"}]
            }]
        }"#;
        let v = parse_chat(body);
        assert_eq!(v["messages"][0]["content"], "hello");
    }

    #[test]
    fn preserves_reasoning_content_on_function_call_roundtrip() {
        let body = br#"{
            "model": "deepseek-v4-pro",
            "input": [
                {"type": "message", "role": "user", "content": "read file"},
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "Need file."}]},
                {"type": "function_call", "call_id": "call_1", "name": "read_file", "arguments": "{\"path\":\"a.md\"}", "reasoning_content": "Need file."},
                {"type": "function_call_output", "call_id": "call_1", "output": "ok"}
            ]
        }"#;
        let v = parse_chat(body);
        let assistant = &v["messages"][1];
        assert_eq!(assistant["role"], "assistant");
        assert_eq!(assistant["reasoning_content"], "Need file.");
        assert_eq!(assistant["tool_calls"][0]["function"]["name"], "read_file");
    }

    #[test]
    fn uses_reasoning_summary_before_tool_call_without_placeholder() {
        let body = br#"{
            "model": "deepseek-v4-pro",
            "input": [
                {"type": "message", "role": "user", "content": "read file"},
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "Need file."}]},
                {"type": "function_call", "call_id": "call_1", "name": "read_file", "arguments": "{\"path\":\"a.md\"}"},
                {"type": "function_call_output", "call_id": "call_1", "output": "ok"}
            ]
        }"#;
        let converted = convert_responses_to_chat_with_provider(
            &axum::body::Bytes::from_static(body),
            Some(&deepseek_provider()),
            0,
        )
        .unwrap();
        let v: Value = serde_json::from_slice(&converted.body).unwrap();
        assert_eq!(v["messages"][1]["reasoning_content"], "Need file.");
        assert_ne!(v["messages"][1]["reasoning_content"], "tool call");
    }

    #[test]
    fn backfill_inherits_reasoning_from_earlier_assistant_message() {
        let body = br#"{
            "model": "deepseek-v4-pro",
            "messages": [
                {"role": "assistant", "content": "", "reasoning_content": "Plan the patch."},
                {"role": "user", "content": "apply it"},
                {"role": "assistant", "content": null, "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "apply_patch", "arguments": "{}"}}
                ]},
                {"role": "tool", "tool_call_id": "call_1", "content": "ok"}
            ]
        }"#;
        let out = super::super::normalize_upstream_body(
            &axum::body::Bytes::from_static(body),
            &deepseek_provider(),
            0,
        );
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["messages"][2]["reasoning_content"], "Plan the patch.");
    }

    #[test]
    fn truncates_large_tool_output_when_enabled() {
        let long_output = "HEAD".to_string() + &"x".repeat(200) + "TAIL";
        let body = format!(
            r#"{{
            "model": "deepseek-v4-pro",
            "messages": [
                {{"role": "assistant", "content": null, "tool_calls": [
                    {{"id": "call_1", "type": "function", "function": {{"name": "grep", "arguments": "{{}}"}}}}
                ]}},
                {{"role": "tool", "tool_call_id": "call_1", "content": {}}}
            ]
        }}"#,
            serde_json::to_string(&long_output).unwrap()
        );
        let body_bytes = axum::body::Bytes::from(body.into_bytes());
        let out = super::super::normalize_upstream_body(&body_bytes, &deepseek_provider(), 80);
        let v: Value = serde_json::from_slice(&out).unwrap();
        let content = v["messages"][1]["content"].as_str().unwrap();
        assert!(content.contains("HEAD"));
        assert!(content.contains("TAIL"));
        assert!(content.contains("truncated"));
        assert!(content.chars().count() < long_output.chars().count());
    }

    #[test]
    fn leaves_tool_output_intact_when_truncation_disabled() {
        let long_output = "a".repeat(500);
        let body = format!(
            r#"{{
            "model": "deepseek-v4-pro",
            "messages": [
                {{"role": "assistant", "content": null, "tool_calls": [
                    {{"id": "call_1", "type": "function", "function": {{"name": "grep", "arguments": "{{}}"}}}}
                ]}},
                {{"role": "tool", "tool_call_id": "call_1", "content": {}}}
            ]
        }}"#,
            serde_json::to_string(&long_output).unwrap()
        );
        let body_bytes = axum::body::Bytes::from(body.into_bytes());
        let out = super::super::normalize_upstream_body(&body_bytes, &deepseek_provider(), 0);
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["messages"][1]["content"], long_output);
    }

    #[test]
    fn merges_multiple_tool_calls_before_outputs() {
        let body = br#"{
            "model": "gpt-5.5",
            "input": [
                {"type": "function_call", "call_id": "call_1", "name": "read_file", "arguments": "{\"path\":\"README.md\"}"},
                {"type": "function_call", "call_id": "call_2", "name": "list_files", "arguments": "{\"path\":\"src\"}"},
                {"type": "function_call_output", "call_id": "call_1", "output": "Readme content"},
                {"type": "function_call_output", "call_id": "call_2", "output": "main.rs"},
                {"type": "message", "role": "user", "content": "Continue"}
            ]
        }"#;
        let v = parse_chat(body);
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0]["tool_calls"].as_array().unwrap().len(), 2);
        assert_eq!(msgs[1]["tool_call_id"], "call_1");
        assert_eq!(msgs[2]["tool_call_id"], "call_2");
    }

    #[test]
    fn converts_function_call_history() {
        let body = br#"{
            "model": "qwen3.7-plus",
            "input": [
                {"type": "message", "role": "user", "content": "open app"},
                {"type": "function_call", "call_id": "call_1", "name": "read_file", "arguments": "{\"path\":\"a.md\"}"},
                {"type": "function_call_output", "call_id": "call_1", "output": "ok"}
            ]
        }"#;
        let v = parse_chat(body);
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[1]["tool_calls"][0]["function"]["name"], "read_file");
        assert_eq!(msgs[2]["role"], "tool");
    }

    #[test]
    fn backfills_missing_tool_responses_for_parallel_calls() {
        let body = br#"{
            "model": "gpt-5.5",
            "input": [
                {"type": "function_call", "call_id": "call_1", "name": "read_file", "arguments": "{}"},
                {"type": "function_call", "call_id": "call_2", "name": "list_files", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "call_1", "output": "ok"},
                {"type": "message", "role": "user", "content": "Continue"}
            ]
        }"#;
        let v = parse_chat(body);
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[1]["tool_call_id"], "call_1");
        assert_eq!(msgs[2]["tool_call_id"], "call_2");
        assert_eq!(msgs[2]["content"], "");
    }

    #[test]
    fn backfills_tool_responses_when_assistant_tool_calls_trail_history() {
        let body = br#"{
            "model": "gpt-5.5",
            "messages": [
                {"role": "user", "content": "run tools"},
                {"role": "assistant", "content": null, "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "a", "arguments": "{}"}},
                    {"id": "call_2", "type": "function", "function": {"name": "b", "arguments": "{}"}}
                ]}
            ]
        }"#;
        let v = parse_chat(body);
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[2]["tool_call_id"], "call_1");
        assert_eq!(msgs[3]["tool_call_id"], "call_2");
    }

    #[test]
    fn collapses_mid_stream_system_messages_to_head() {
        let body = br#"{
            "model": "minimax-m3",
            "messages": [
                {"role": "system", "content": "base"},
                {"role": "user", "content": "hi"},
                {"role": "system", "content": "reminder"},
                {"role": "assistant", "content": "ok"}
            ]
        }"#;
        let v = parse_chat(body);
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "base\n\nreminder");
        assert_eq!(msgs[1]["role"], "user");
    }

    #[test]
    fn maps_latest_reminder_role_to_user() {
        assert_eq!(map_role_for_upstream("latest_reminder"), "user");
        assert_eq!(map_role_for_upstream("developer"), "system");
    }

    #[test]
    fn strips_tool_choice_when_tools_are_absent() {
        let body = br#"{
            "model": "gpt-5.5",
            "stream": false,
            "tool_choice": "auto",
            "parallel_tool_calls": true,
            "input": [{"role": "user", "content": "hi"}]
        }"#;
        let v = parse_chat(body);
        assert!(v.get("tool_choice").is_none());
        assert!(v.get("parallel_tool_calls").is_none());
    }

    #[test]
    fn injects_stream_options_include_usage_for_streaming_requests() {
        let body = br#"{
            "model": "gpt-5.5",
            "stream": true,
            "input": [{"role": "user", "content": "hi"}]
        }"#;
        let v = parse_chat(body);
        assert_eq!(v["stream_options"]["include_usage"], true);
    }

    #[test]
    fn canonicalizes_empty_tool_arguments_to_object() {
        let body = br#"{
            "model": "minimax-m3",
            "input": [
                {"type": "function_call", "call_id": "call_1", "name": "noop", "arguments": ""},
                {"type": "function_call_output", "call_id": "call_1", "output": "ok"}
            ]
        }"#;
        let v = parse_chat(body);
        assert_eq!(v["messages"][0]["tool_calls"][0]["function"]["arguments"], "{}");
    }

    #[test]
    fn preserves_web_search_call_as_synthetic_tool_history() {
        let body = br#"{
            "model": "gpt-5.4",
            "input": [
                {"type": "message", "role": "user", "content": "search rust async"},
                {
                    "type": "web_search_call",
                    "id": "ws_1",
                    "status": "completed",
                    "action": {"type": "search", "query": "rust async tutorial"}
                },
                {"type": "message", "role": "user", "content": "continue"}
            ]
        }"#;
        let v = parse_chat(body);
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(
            msgs[1]["tool_calls"][0]["function"]["name"],
            "codex_builtin__web_search"
        );
        assert_eq!(msgs[1]["tool_calls"][0]["id"], "ws_1");
        assert_eq!(msgs[2]["role"], "tool");
        assert_eq!(msgs[2]["tool_call_id"], "ws_1");
        assert!(msgs[1]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap()
            .contains("rust async tutorial"));
        assert!(msgs[2]["content"].as_str().unwrap().contains("completed"));
        assert_eq!(msgs[3]["content"], "continue");
    }

    #[test]
    fn builtin_tool_history_avoids_duplicate_web_search_payload() {
        let body = br#"{
            "model": "gpt-5.4",
            "input": [{
                "type": "web_search_call",
                "id": "ws_big",
                "status": "completed",
                "action": {
                    "type": "search",
                    "query": "rust async tutorial",
                    "sources": [
                        {"type": "url", "url": "https://example.com/a", "title": "A"},
                        {"type": "url", "url": "https://example.com/b", "title": "B"}
                    ]
                }
            }]
        }"#;
        let v = parse_chat(body);
        let args = v["messages"][0]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        let tool_content = v["messages"][1]["content"].as_str().unwrap();
        assert!(args.contains("rust async tutorial"));
        assert!(!args.contains("example.com/a"));
        assert!(tool_content.contains("example.com/a"));
        assert!(tool_content.contains("example.com/b"));
    }

    #[test]
    fn preserves_file_search_call_as_synthetic_tool_history() {
        let body = br#"{
            "model": "gpt-5.4",
            "input": [
                {
                    "type": "file_search_call",
                    "id": "fs_1",
                    "status": "completed",
                    "queries": ["project readme", "setup guide"]
                }
            ]
        }"#;
        let v = parse_chat(body);
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(
            msgs[0]["tool_calls"][0]["function"]["name"],
            "codex_builtin__file_search"
        );
        assert!(msgs[0]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap()
            .contains("project readme"));
        assert_eq!(msgs[1]["tool_call_id"], "fs_1");
    }

    #[test]
    fn preserves_local_shell_call_and_output_sequence() {
        let body = br#"{
            "model": "gpt-5.4",
            "input": [
                {
                    "type": "local_shell_call",
                    "call_id": "shell_1",
                    "status": "completed",
                    "action": {"command": ["cargo", "test"]}
                },
                {
                    "type": "local_shell_call_output",
                    "call_id": "shell_1",
                    "output": "ok. 91 passed"
                },
                {"type": "message", "role": "user", "content": "next"}
            ]
        }"#;
        let v = parse_chat(body);
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(
            msgs[0]["tool_calls"][0]["function"]["name"],
            "codex_builtin__local_shell"
        );
        assert_eq!(msgs[1]["tool_call_id"], "shell_1");
        assert_eq!(msgs[1]["content"], "ok. 91 passed");
        let args = msgs[0]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        assert!(args.contains("cargo"));
        assert!(!args.contains("91 passed"));
    }

    #[test]
    fn preserves_item_reference_as_user_context() {
        let body = br#"{
            "model": "gpt-5.4",
            "input": [
                {"type": "item_reference", "id": "msg_prev_1"},
                {"type": "message", "role": "user", "content": "go on"}
            ]
        }"#;
        let v = parse_chat(body);
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(
            msgs[0]["content"],
            "[Referenced prior item: msg_prev_1]"
        );
        assert_eq!(msgs[1]["content"], "go on");
    }

    #[test]
    fn repairs_consecutive_assistant_tool_calls_in_messages_array() {
        let body = br#"{
            "model": "gpt-5.5",
            "messages": [
                {"role": "assistant", "content": null, "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "a", "arguments": "{}"}}]},
                {"role": "assistant", "content": null, "tool_calls": [{"id": "call_2", "type": "function", "function": {"name": "b", "arguments": "{}"}}]},
                {"role": "tool", "tool_call_id": "call_1", "content": "1"},
                {"role": "tool", "tool_call_id": "call_2", "content": "2"}
            ]
        }"#;
        let v = parse_chat(body);
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["tool_calls"].as_array().unwrap().len(), 2);
    }
}
