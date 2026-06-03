//! Responses API 请求 → Chat Completions 请求（保留 tools / 多轮 tool 历史）。

use serde_json::{json, Value};

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
        match input {
            Value::String(text) => {
                messages.push(json!({
                    "role": "user",
                    "content": text,
                }));
            }
            Value::Array(items) => {
                for item in items {
                    messages.extend(convert_input_item(item)?);
                }
            }
            _ => {}
        }
    }

    Ok(messages)
}

fn convert_input_item(item: &Value) -> anyhow::Result<Vec<Value>> {
    if let Some(text) = item.as_str() {
        return Ok(vec![json!({
            "role": "user",
            "content": text,
        })]);
    }

    let Some(obj) = item.as_object() else {
        return Ok(Vec::new());
    };

    match obj.get("type").and_then(|v| v.as_str()).unwrap_or("message") {
        "message" => {
            let role = obj
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("user");
            let content = obj
                .get("content")
                .map(normalize_content_for_upstream)
                .unwrap_or(Value::Null);
            Ok(vec![json!({
                "role": map_role_for_upstream(role),
                "content": content,
            })])
        }
        "function_call" => {
            let call_id = obj
                .get("call_id")
                .or_else(|| obj.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("call_unknown");
            let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let arguments = obj
                .get("arguments")
                .map(|v| {
                    if v.is_string() {
                        v.as_str().unwrap_or("{}").to_string()
                    } else {
                        v.to_string()
                    }
                })
                .unwrap_or_else(|| "{}".to_string());
            Ok(vec![json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": call_id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": arguments,
                    }
                }]
            })])
        }
        "function_call_output" | "tool_result" => {
            let call_id = obj
                .get("call_id")
                .or_else(|| obj.get("tool_call_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("call_unknown");
            let output = obj
                .get("output")
                .or_else(|| obj.get("content"))
                .map(|v| {
                    if v.is_string() {
                        v.as_str().unwrap_or("").to_string()
                    } else {
                        v.to_string()
                    }
                })
                .unwrap_or_default();
            Ok(vec![json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": output,
            })])
        }
        _ => {
            if obj.contains_key("role") {
                let role = obj.get("role").and_then(|v| v.as_str()).unwrap_or("user");
                let content = obj
                    .get("content")
                    .map(normalize_content_for_upstream)
                    .unwrap_or(Value::Null);
                Ok(vec![json!({
                    "role": map_role_for_upstream(role),
                    "content": content,
                })])
            } else {
                Ok(Vec::new())
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

fn map_role_for_upstream(role: &str) -> &str {
    match role {
        "developer" | "latest_reminder" => "system",
        _ => role,
    }
}

fn normalize_messages_for_upstream(messages: &mut [Value]) {
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

fn normalize_content_for_upstream(content: &Value) -> Value {
    match content {
        Value::String(_) | Value::Null => content.clone(),
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                if let Some(text) = item.as_str() {
                    parts.push(text.to_string());
                    continue;
                }
                let Some(obj) = item.as_object() else {
                    continue;
                };
                let item_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match item_type {
                    "input_text" | "output_text" | "text" => {
                        if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                            parts.push(text.to_string());
                        }
                    }
                    "input_image" | "image" => {
                        if let Some(url) = obj
                            .get("image_url")
                            .and_then(|v| v.get("url"))
                            .and_then(|v| v.as_str())
                            .or_else(|| obj.get("url").and_then(|v| v.as_str()))
                        {
                            parts.push(format!("[image: {url}]"));
                        }
                    }
                    _ => {
                        if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                            parts.push(text.to_string());
                        }
                    }
                }
            }
            if parts.is_empty() {
                content.clone()
            } else {
                Value::String(parts.join("\n"))
            }
        }
        _ => content.clone(),
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
}
