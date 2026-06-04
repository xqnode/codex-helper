//! Responses API 请求 → Chat Completions 请求（保留 tools / 多轮 tool 历史）。

use serde_json::{json, Value};

use super::codex_compat::{
    append_reasoning_content, extract_reasoning_field_text, extract_reasoning_summary_text,
};

const TOOL_SEARCH_PROXY_NAME: &str = "tool_search";
const CUSTOM_TOOL_INPUT_FIELD: &str = "input";
const REASONING_PLACEHOLDER: &str = "tool call";

pub fn convert_responses_to_chat(body: &axum::body::Bytes) -> anyhow::Result<Vec<u8>> {
    let value: Value = serde_json::from_slice(body)?;
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
        extract_messages_from_responses(&value)?
    };

    if messages.is_empty() {
        anyhow::bail!("无法从 Responses 请求中提取 input/instructions");
    }

    normalize_messages_for_upstream(&mut messages);
    repair_chat_tool_message_sequence(&mut messages);
    backfill_tool_call_reasoning_placeholders(&mut messages);

    let mut chat = json!({
        "model": model,
        "messages": messages,
        "stream": stream,
    });

    if let Some(tools) = convert_tools(value.get("tools")) {
        chat["tools"] = tools;
    }
    if let Some(tool_choice) = value.get("tool_choice") {
        chat["tool_choice"] = tool_choice.clone();
    }
    if let Some(max_tokens) = value
        .get("max_output_tokens")
        .or_else(|| value.get("max_tokens"))
    {
        chat["max_tokens"] = max_tokens.clone();
    }
    for key in ["temperature", "top_p", "presence_penalty", "frequency_penalty"] {
        if let Some(v) = value.get(key) {
            chat[key] = v.clone();
        }
    }

    Ok(serde_json::to_vec(&chat)?)
}

fn extract_messages_from_responses(value: &Value) -> anyhow::Result<Vec<Value>> {
    let mut messages = Vec::new();

    if let Some(instructions) = value.get("instructions").and_then(|v| v.as_str()) {
        if !instructions.trim().is_empty() {
            messages.push(json!({
                "role": "system",
                "content": instructions,
            }));
        }
    }

    if let Some(input) = value.get("input") {
        append_responses_input_as_chat_messages(input, &mut messages)?;
    }

    Ok(messages)
}

fn append_responses_input_as_chat_messages(
    input: &Value,
    messages: &mut Vec<Value>,
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
) -> anyhow::Result<()> {
    let item_type = item.get("type").and_then(|v| v.as_str());
    match item_type {
        Some("function_call") => {
            append_unique_pending_reasoning(pending_reasoning, responses_item_reasoning_text(item));
            pending_tool_calls.push(responses_function_call_to_chat_tool_call(item));
        }
        Some("custom_tool_call") => {
            append_unique_pending_reasoning(pending_reasoning, responses_item_reasoning_text(item));
            pending_tool_calls.push(responses_custom_tool_call_to_chat_tool_call(item));
        }
        Some("tool_search_call") => {
            append_unique_pending_reasoning(pending_reasoning, responses_item_reasoning_text(item));
            pending_tool_calls.push(responses_tool_search_call_to_chat_tool_call(item));
        }
        Some("function_call_output") => {
            flush_pending_tool_calls(
                messages,
                pending_tool_calls,
                pending_reasoning,
                last_assistant_index,
            );
            let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
            let output = match item.get("output") {
                Some(Value::String(s)) => s.clone(),
                Some(v) => canonical_json_string(v),
                None => String::new(),
            };
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": output,
            }));
        }
        Some("custom_tool_call_output") | Some("tool_search_output") => {
            flush_pending_tool_calls(
                messages,
                pending_tool_calls,
                pending_reasoning,
                last_assistant_index,
            );
            let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
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
        Some(
            "web_search_call"
            | "file_search_call"
            | "item_reference"
            | "image_generation_call"
            | "code_interpreter_call"
            | "local_shell_call"
            | "local_shell_call_output",
        ) => {}
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
        .map(normalize_content_for_upstream)
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

fn responses_function_call_to_chat_tool_call(item: &Value) -> Value {
    let call_id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let arguments = canonicalize_tool_arguments(item.get("arguments"));

    json!({
        "id": call_id,
        "type": "function",
        "function": {
            "name": name,
            "arguments": arguments,
        }
    })
}

fn responses_custom_tool_call_to_chat_tool_call(item: &Value) -> Value {
    let call_id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let input = item.get("input").cloned().unwrap_or_else(|| json!(""));

    json!({
        "id": call_id,
        "type": "function",
        "function": {
            "name": name,
            "arguments": canonical_json_string(&json!({ CUSTOM_TOOL_INPUT_FIELD: input })),
        }
    })
}

fn responses_tool_search_call_to_chat_tool_call(item: &Value) -> Value {
    let call_id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let arguments = item
        .get("arguments")
        .map(canonical_json_string)
        .unwrap_or_else(|| "{}".to_string());

    json!({
        "id": call_id,
        "type": "function",
        "function": {
            "name": TOOL_SEARCH_PROXY_NAME,
            "arguments": arguments,
        }
    })
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

fn canonicalize_tool_arguments(arguments: Option<&Value>) -> String {
    match arguments {
        Some(Value::String(s)) => s.clone(),
        Some(v) => canonical_json_string(v),
        None => "{}".to_string(),
    }
}

fn canonical_json_string(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        text.to_string()
    } else {
        serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
    }
}

/// 合并连续的 assistant+tool_calls 消息，避免上游报 insufficient tool messages。
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

fn backfill_tool_call_reasoning_placeholders(messages: &mut [Value]) {
    for message in messages.iter_mut() {
        if !assistant_has_tool_calls(message) {
            continue;
        }
        let has_reasoning = message
            .get("reasoning_content")
            .and_then(|v| v.as_str())
            .is_some_and(|text| !text.trim().is_empty());
        if !has_reasoning {
            if let Some(obj) = message.as_object_mut() {
                obj.insert(
                    "reasoning_content".into(),
                    Value::String(REASONING_PLACEHOLDER.to_string()),
                );
            }
        }
    }
}

fn convert_tools(tools: Option<&Value>) -> Option<Value> {
    let arr = tools?.as_array()?;
    let mut out = Vec::new();
    for tool in arr {
        if let Some(converted) = convert_one_tool(tool) {
            out.push(converted);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(Value::Array(out))
    }
}

fn convert_one_tool(tool: &Value) -> Option<Value> {
    let tool_type = tool.get("type").and_then(|v| v.as_str()).unwrap_or("function");
    if tool_type != "function" {
        return None;
    }

    if tool.get("function").is_some() {
        return Some(tool.clone());
    }

    let name = tool.get("name").and_then(|v| v.as_str())?;
    Some(json!({
        "type": "function",
        "function": {
            "name": name,
            "description": tool.get("description").cloned().unwrap_or(json!("")),
            "parameters": tool.get("parameters").cloned().unwrap_or(json!({
                "type": "object",
                "properties": {}
            })),
        }
    }))
}

pub fn map_role_for_upstream(role: &str) -> &str {
    match role {
        "developer" | "latest_reminder" => "system",
        _ => role,
    }
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

fn extract_text_from_content_part(obj: &serde_json::Map<String, Value>) -> Option<String> {
    let item_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match item_type {
        "input_text" | "output_text" | "text" | "summarized_text" => obj
            .get("text")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        "input_image" | "image" | "input_file" | "input_audio" | "input_video" | "image_url" => {
            None
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
            obj.insert("content".into(), normalize_content_for_upstream(&content));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let out = convert_responses_to_chat(&axum::body::Bytes::from_static(body)).unwrap();
        let v: Value = serde_json::from_slice(&out).unwrap();
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
        let out = convert_responses_to_chat(&axum::body::Bytes::from_static(body)).unwrap();
        let v: Value = serde_json::from_slice(&out).unwrap();
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
        let out = convert_responses_to_chat(&axum::body::Bytes::from_static(body)).unwrap();
        let v: Value = serde_json::from_slice(&out).unwrap();
        let assistant = &v["messages"][1];
        assert_eq!(assistant["role"], "assistant");
        assert_eq!(assistant["reasoning_content"], "Need file.");
        assert_eq!(assistant["tool_calls"][0]["function"]["name"], "read_file");
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
        let out = convert_responses_to_chat(&axum::body::Bytes::from_static(body)).unwrap();
        let v: Value = serde_json::from_slice(&out).unwrap();
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
        let out = convert_responses_to_chat(&axum::body::Bytes::from_static(body)).unwrap();
        let v: Value = serde_json::from_slice(&out).unwrap();
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[1]["tool_calls"][0]["function"]["name"], "read_file");
        assert_eq!(msgs[2]["role"], "tool");
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
        let out = convert_responses_to_chat(&axum::body::Bytes::from_static(body)).unwrap();
        let v: Value = serde_json::from_slice(&out).unwrap();
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["tool_calls"].as_array().unwrap().len(), 2);
    }
}
