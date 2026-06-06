//! 将上游 / 代理错误包装为 Codex Responses API 可识别的 `failed` 形态。

use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    body::Body,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use serde_json::{json, Value};

pub fn responses_failed_value(
    message: &str,
    error_type: Option<&str>,
    model: Option<&str>,
) -> Value {
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let mut error = json!({ "message": message });
    if let Some(error_type) = error_type.filter(|value| !value.is_empty()) {
        error["type"] = json!(error_type);
    }

    json!({
        "id": "resp_helper_failed",
        "object": "response",
        "created_at": created_at,
        "status": "failed",
        "model": model.unwrap_or(""),
        "output": [],
        "error": error,
        "usage": {
            "input_tokens": 0,
            "output_tokens": 0,
            "total_tokens": 0,
            "output_tokens_details": { "reasoning_tokens": 0 }
        }
    })
}

pub fn responses_failed_json(
    message: &str,
    error_type: Option<&str>,
    model: Option<&str>,
) -> String {
    serde_json::to_string(&responses_failed_value(message, error_type, model))
        .unwrap_or_else(|_| r#"{"status":"failed","error":{"message":"upstream error"}}"#.into())
}

pub fn responses_failed_sse(message: &str, error_type: Option<&str>, model: Option<&str>) -> Bytes {
    let payload = json!({
        "type": "response.failed",
        "response": responses_failed_value(message, error_type, model),
    });
    let data = serde_json::to_string(&payload).unwrap_or_default();
    Bytes::from(format!("event: response.failed\ndata: {data}\n\n"))
}

pub fn extract_upstream_error_message(bytes: &[u8]) -> (String, Option<String>) {
    if let Ok(value) = serde_json::from_slice::<Value>(bytes) {
        if let Some(error) = value.get("error") {
            let message = error
                .get("message")
                .or_else(|| error.get("detail"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| error.to_string());
            let error_type = error
                .get("type")
                .or_else(|| error.get("code"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            return (message, error_type);
        }
        if let Some(message) = value.get("message").and_then(|v| v.as_str()) {
            let error_type = value
                .get("type")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            return (message.to_string(), error_type);
        }
    }

    if bytes.is_empty() {
        return (
            "上游响应体为空".into(),
            Some("upstream_error".into()),
        );
    }
    if bytes.starts_with(b"<") || bytes.windows(5).any(|window| window == b"<!DOC" || window == b"<html")
    {
        return (
            "上游返回了 HTML 页面，请确认 Base URL 是否应以 /v1 结尾，例如 http://host:8080/v1"
                .into(),
            Some("upstream_error".into()),
        );
    }

    let preview = String::from_utf8_lossy(bytes);
    let trimmed: String = preview.chars().take(500).collect();
    (trimmed, Some("upstream_error".into()))
}

pub fn responses_failed_http_response(
    status: StatusCode,
    stream_request: bool,
    message: &str,
    error_type: Option<&str>,
    model: &str,
) -> Response {
    if stream_request {
        return (
            StatusCode::OK,
            [
                (
                    axum::http::header::CONTENT_TYPE.as_str(),
                    "text/event-stream; charset=utf-8",
                ),
                (axum::http::header::CACHE_CONTROL.as_str(), "no-cache"),
            ],
            Body::from(responses_failed_sse(message, error_type, Some(model))),
        )
            .into_response();
    }

    (
        status,
        [(axum::http::header::CONTENT_TYPE.as_str(), "application/json")],
        responses_failed_json(message, error_type, Some(model)),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_json_uses_response_object_shape() {
        let value: Value = serde_json::from_str(&responses_failed_json(
            "quota exceeded",
            Some("rate_limit_error"),
            Some("deepseek-v4-flash"),
        ))
        .unwrap();
        assert_eq!(value["status"], "failed");
        assert_eq!(value["object"], "response");
        assert_eq!(value["error"]["message"], "quota exceeded");
        assert_eq!(value["error"]["type"], "rate_limit_error");
        assert_eq!(value["model"], "deepseek-v4-flash");
    }

    #[test]
    fn failed_sse_emits_response_failed_event() {
        let sse = String::from_utf8(responses_failed_sse("boom", Some("upstream_error"), None).to_vec())
            .unwrap();
        assert!(sse.contains("event: response.failed"));
        assert!(sse.contains("\"status\":\"failed\""));
        assert!(sse.contains("boom"));
    }

    #[test]
    fn extracts_chat_style_upstream_error() {
        let (message, error_type) = extract_upstream_error_message(
            br#"{"error":{"message":"invalid key","type":"authentication_error"}}"#,
        );
        assert_eq!(message, "invalid key");
        assert_eq!(error_type.as_deref(), Some("authentication_error"));
    }
}
