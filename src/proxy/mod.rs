use std::sync::{Arc, RwLock as StdRwLock};

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Json, Router,
};
use reqwest::Client;
use tokio::sync::RwLock;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

use crate::config::{self, AppConfig};

mod codex_compat;
mod logged_stream;
mod responses_to_chat;
use logged_stream::LoggingByteStream;
use responses_to_chat::{convert_responses_to_chat, normalize_messages_for_upstream};
use crate::logs::{logs_bootstrap, logs_clear, logs_page};
use crate::request_log::{
    extract_model_from_body, parse_usage_from_json, PendingRequest, RequestLogStore,
};
use crate::settings::{brand_icon_svg, settings_bootstrap, settings_clear_all, settings_page, settings_save, settings_test};

type TrayHealthCheckHook = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
pub struct ProxyState {
    pub config: Arc<RwLock<AppConfig>>,
    pub client: Client,
    pub request_log: RequestLogStore,
    tray_health_check: Arc<StdRwLock<Option<TrayHealthCheckHook>>>,
}

pub fn spawn_server(config: AppConfig) -> anyhow::Result<Arc<ProxyState>> {
    let state = Arc::new(ProxyState {
        config: Arc::new(RwLock::new(config.clone())),
        client: config::build_upstream_client(std::time::Duration::from_secs(300))
            .expect("failed to build HTTP client"),
        request_log: RequestLogStore::new(),
        tray_health_check: Arc::new(StdRwLock::new(None)),
    });
    let addr = format!("{}:{}", config.proxy.host, config.proxy.port);
    let serve_state = state.clone();
    tokio::spawn(async move {
        if let Err(err) = run_listener(serve_state, &addr).await {
            tracing::error!("代理异常退出: {err:#}");
        }
    });
    Ok(state)
}

/// 通知正在运行的代理从磁盘重新加载 config.json（CLI 切换模型后调用）。
pub async fn notify_running_proxy_reload(app: &AppConfig) -> bool {
    let url = format!(
        "http://{}:{}/admin/reload",
        app.proxy.host, app.proxy.port
    );
    let client = match Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    match client.post(&url).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

pub async fn reload_config_in_state(state: &ProxyState) -> anyhow::Result<AppConfig> {
    let app = AppConfig::load()?;
    let mut cfg = state.config.write().await;
    *cfg = app.clone();
    Ok(app)
}

/// 托盘启动后注册；设置页保存 Key 后自动触发连接检测并刷新菜单。
pub fn register_tray_health_check(state: &Arc<ProxyState>, hook: TrayHealthCheckHook) {
    if let Ok(mut slot) = state.tray_health_check.write() {
        *slot = Some(hook);
    }
}

pub fn request_tray_health_check(state: &ProxyState) {
    let hook = state
        .tray_health_check
        .read()
        .ok()
        .and_then(|slot| slot.clone());
    if let Some(hook) = hook {
        hook();
    }
}

pub async fn start_server(config: AppConfig) -> anyhow::Result<()> {
    let addr = format!("{}:{}", config.proxy.host, config.proxy.port);
    let state = ProxyState {
        config: Arc::new(RwLock::new(config.clone())),
        client: config::build_upstream_client(std::time::Duration::from_secs(300))?,
        request_log: RequestLogStore::new(),
        tray_health_check: Arc::new(StdRwLock::new(None)),
    };
    run_listener(Arc::new(state), &addr).await
}

async fn run_listener(state: Arc<ProxyState>, addr: &str) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/health", get(health))
        .route("/admin/reload", post(admin_reload))
        .route("/admin/settings", get(settings_page))
        .route("/admin/brand-icon.svg", get(brand_icon_svg))
        .route("/admin/settings/bootstrap", get(settings_bootstrap))
        .route("/admin/settings/save", post(settings_save))
        .route("/admin/settings/clear-all", post(settings_clear_all))
        .route("/admin/settings/test", post(settings_test))
        .route("/admin/logs", get(logs_page))
        .route("/admin/logs/bootstrap", get(logs_bootstrap))
        .route("/admin/logs/clear", post(logs_clear))
        .route("/v1/models", get(list_models))
        .route("/v1/chat/completions", post(proxy_chat))
        .route("/v1/responses", post(proxy_responses))
        .fallback(any(catch_all))
        .layer(TraceLayer::new_for_http())
        .with_state(state.as_ref().clone());

    info!("Codex Helper 代理已启动: http://{addr}/v1");

    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        anyhow::anyhow!("无法绑定端口 {addr}: {e}。请检查端口是否被占用。")
    })?;

    axum::serve(listener, app).await?;
    Ok(())
}

async fn admin_reload(State(state): State<ProxyState>) -> impl IntoResponse {
    match reload_config_in_state(&state).await {
        Ok(app) => {
            let provider_name = app
                .active_provider()
                .map(|p| p.name.clone())
                .unwrap_or_else(|_| "unknown".into());
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "ok",
                    "active": app.active,
                    "provider": provider_name,
                })),
            )
                .into_response()
        }
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn health(State(state): State<ProxyState>) -> impl IntoResponse {
    let config = state.config.read().await;
    let active = config.active.clone();
    let provider = config
        .active_provider()
        .map(|p| p.name.clone())
        .unwrap_or_else(|_| "unknown".into());
    axum::Json(serde_json::json!({
        "status": "ok",
        "active": active,
        "provider": provider,
    }))
}

async fn list_models(State(state): State<ProxyState>) -> impl IntoResponse {
    let config = state.config.read().await;
    let provider = match config.active_provider() {
        Ok(p) => p,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({ "error": err.to_string() })),
            )
                .into_response();
        }
    };

    axum::Json(serde_json::json!({
        "object": "list",
        "data": [{
            "id": provider.default_model,
            "object": "model",
            "owned_by": provider.id,
        }]
    }))
    .into_response()
}

async fn proxy_chat(
    State(state): State<ProxyState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    forward_request(&state, "/chat/completions", Method::POST, headers, body).await
}

async fn proxy_responses(
    State(state): State<ProxyState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    match convert_responses_to_chat(&body) {
        Ok(chat_body) => forward_responses_request(&state, headers, chat_body.into()).await,
        Err(err) => {
            warn!("Responses 请求转换失败: {err}");
            (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({
                    "error": {
                        "message": format!("Responses 请求转换失败: {err}"),
                        "type": "invalid_request_error"
                    }
                })),
            )
                .into_response()
        }
    }
}

async fn forward_responses_request(
    state: &ProxyState,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let response = forward_request(
        state,
        "/chat/completions",
        Method::POST,
        headers,
        body,
    )
    .await;
    convert_chat_response_to_responses(response).await
}

async fn catch_all(
    State(state): State<ProxyState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let path = uri.path();
    if !path.starts_with("/v1/") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let upstream_path = path.trim_start_matches("/v1");
    forward_request(&state, upstream_path, method, headers, body).await
}

async fn forward_request(
    state: &ProxyState,
    upstream_path: &str,
    method: Method,
    _headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let started = std::time::Instant::now();
    let config = state.config.read().await.clone();
    let provider = match config.active_provider() {
        Ok(p) => p.clone(),
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({ "error": err.to_string() })),
            )
                .into_response();
        }
    };

    let rewritten_body = if body.is_empty() {
        None
    } else {
        Some(normalize_upstream_body(&body, &provider))
    };
    let model = rewritten_body
        .as_ref()
        .map(|bytes| extract_model_from_body(bytes, provider.upstream_model()))
        .unwrap_or_else(|| provider.upstream_model().to_string());
    let pending_base = PendingRequest {
        provider_id: provider.id.clone(),
        provider_name: provider.name.clone(),
        model,
        path: upstream_path.to_string(),
        stream: false,
        started,
        status: 0,
    };

    let api_key = match config::resolve_api_key(&provider.api_key_env) {
        Ok(key) => key,
        Err(err) => {
            let mut pending = pending_base;
            pending.status = StatusCode::UNAUTHORIZED.as_u16();
            let entry = state.request_log.finalize(pending, None);
            state.request_log.push(entry).await;
            return (
                StatusCode::UNAUTHORIZED,
                axum::Json(serde_json::json!({
                    "error": {
                        "message": err.to_string(),
                        "type": "authentication_error"
                    }
                })),
            )
                .into_response();
        }
    };

    let target = format!(
        "{}{}",
        provider.base_url.trim_end_matches('/'),
        upstream_path
    );

    let mut request = state.client.request(method, &target);
    request = request.header("Authorization", format!("Bearer {api_key}"));
    request = request.header("Content-Type", "application/json");
    // 不转发 Codex 客户端请求头（Accept / OpenAI-Beta 等），避免中转站报 Unsupported content type

    if let Some(rewritten) = rewritten_body {
        request = request.body(rewritten);
    }

    match request.send().await {
        Ok(resp) => {
            let status = resp.status();
            let mut pending = pending_base;
            pending.status = status.as_u16();
            let mut response_headers = HeaderMap::new();
            let mut is_sse = false;
            for (name, value) in resp.headers() {
                if name == reqwest::header::TRANSFER_ENCODING {
                    continue;
                }
                if name == reqwest::header::CONTENT_TYPE {
                    if let Ok(v) = value.to_str() {
                        if v.to_ascii_lowercase().contains("text/event-stream") {
                            is_sse = true;
                        }
                    }
                }
                if let Ok(v) = HeaderValue::from_bytes(value.as_bytes()) {
                    response_headers.insert(name, v);
                }
            }

            if is_sse {
                pending.stream = true;
                response_headers.remove(reqwest::header::CONTENT_LENGTH);
                let stream = LoggingByteStream::new(
                    resp.bytes_stream(),
                    pending,
                    state.request_log.clone(),
                );
                let body = Body::from_stream(stream);
                (status, response_headers, body).into_response()
            } else {
                let bytes = resp.bytes().await.unwrap_or_default();
                let usage = serde_json::from_slice::<serde_json::Value>(&bytes)
                    .ok()
                    .and_then(|value| parse_usage_from_json(&value));
                let entry = state.request_log.finalize(pending, usage);
                state.request_log.push(entry).await;
                (status, response_headers, Body::from(bytes)).into_response()
            }
        }
        Err(err) => {
            warn!("上游请求失败: {target} -> {err}");
            let mut pending = pending_base;
            pending.status = StatusCode::BAD_GATEWAY.as_u16();
            let entry = state.request_log.finalize(pending, None);
            state.request_log.push(entry).await;
            (
                StatusCode::BAD_GATEWAY,
                axum::Json(serde_json::json!({
                    "error": {
                        "message": format!("上游请求失败: {err}"),
                        "type": "upstream_error"
                    }
                })),
            )
                .into_response()
        }
    }
}

async fn convert_chat_response_to_responses(response: Response) -> Response {
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if !status.is_success() {
        return response;
    }

    // 流式分支：上游返回 chat completions SSE chunks，需要翻译成
    // Responses API SSE 事件（带 event: response.created / response.output_text.delta /
    // response.completed 等），否则 Codex Desktop 会报 missing field `type`
    // 然后陷入 Reconnecting... 循环。翻译逻辑移植自 cc-switch。
    if content_type.contains("text/event-stream") {
        use futures_util::TryStreamExt;

        let (mut parts, body) = response.into_parts();
        // 把上游字节流喂给 cc_switch 翻译器，输出是一系列 Responses API SSE
        // 事件（每条都是 event: xxx\ndata: {...}\n\n 形式）。
        let upstream_stream = body
            .into_data_stream()
            .map_err(std::io::Error::other);
        let translated = codex_compat::create_responses_sse_stream_from_chat(upstream_stream);
        parts.headers.remove(reqwest::header::CONTENT_LENGTH);
        parts.headers.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream; charset=utf-8"),
        );
        parts.headers.insert(
            reqwest::header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache"),
        );
        return Response::from_parts(parts, Body::from_stream(translated)).into_response();
    }

    let body = response.into_body();
    let bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(err) => {
            warn!("读取上游响应失败: {err}");
            return (
                StatusCode::BAD_GATEWAY,
                axum::Json(serde_json::json!({
                    "error": {
                        "message": format!("读取上游响应失败: {err}"),
                        "type": "upstream_error"
                    }
                })),
            )
                .into_response();
        }
    };

    match chat_json_to_responses_json(&bytes) {
        Ok(converted) => (
            status,
            [(reqwest::header::CONTENT_TYPE.as_str(), "application/json")],
            converted,
        )
            .into_response(),
        Err(err) => {
            let hint = if bytes.starts_with(b"<") || bytes.windows(5).any(|w| w == b"<!DOC" || w == b"<html") {
                "（上游返回了 HTML 页面，请确认 Base URL 是否应以 /v1 结尾，例如 http://host:8080/v1）"
            } else if bytes.is_empty() {
                "（上游响应体为空）"
            } else {
                ""
            };
            warn!("Chat 响应转换 Responses 失败: {err}{hint}");
            (
                StatusCode::BAD_GATEWAY,
                axum::Json(serde_json::json!({
                    "error": {
                        "message": format!("Chat 响应转换 Responses 失败: {err}{hint}"),
                        "type": "upstream_error"
                    }
                })),
            )
                .into_response()
        }
    }
}

fn chat_json_to_responses_json(bytes: &[u8]) -> anyhow::Result<String> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let id = value
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("resp_codex_helper");
    let model = value
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("deepseek-chat");
    let created_at = value
        .get("created")
        .and_then(|v| v.as_i64())
        .unwrap_or_else(unix_timestamp_now);
    let output_text = value
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .unwrap_or_default();

    let usage = value.get("usage").cloned().unwrap_or_else(|| {
        serde_json::json!({
            "input_tokens": 0,
            "output_tokens": 0,
            "total_tokens": 0,
        })
    });

    let response = serde_json::json!({
        "id": id,
        "object": "response",
        "created_at": created_at,
        "status": "completed",
        "model": model,
        "output": [{
            "id": format!("msg_{id}"),
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": output_text,
                "annotations": []
            }]
        }],
        "output_text": output_text,
        "usage": usage,
    });
    Ok(serde_json::to_string(&response)?)
}

fn unix_timestamp_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_developer_role_to_system() {
        let body = br#"{"model":"deepseek-v4-flash","messages":[{"role":"developer","content":[{"type":"input_text","text":"hi"}]}]}"#;
        let out = normalize_upstream_body(
            &axum::body::Bytes::from_static(body),
            &config::ProviderConfig {
                id: "deepseek".into(),
                name: "DeepSeek".into(),
                base_url: "https://api.deepseek.com/v1".into(),
                api_key_env: "DEEPSEEK_API_KEY".into(),
                default_model: "deepseek-v4-flash".into(),
                api_model: "deepseek-v4-flash".into(),
                wire_api: "responses".into(),
            },
        );
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["messages"][0]["role"], "system");
        assert_eq!(v["messages"][0]["content"], "hi");
        assert_eq!(v["model"], "deepseek-v4-flash");
    }

    #[test]
    fn wraps_chat_response_as_responses_json() {
        let body = br#"{"id":"chatcmpl_1","object":"chat.completion","created":123,"model":"deepseek-chat","choices":[{"message":{"role":"assistant","content":"pong"}}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
        let out = chat_json_to_responses_json(body).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["object"], "response");
        assert_eq!(v["status"], "completed");
        assert_eq!(v["output_text"], "pong");
        assert_eq!(v["output"][0]["content"][0]["type"], "output_text");
    }
}

fn normalize_upstream_body(body: &axum::body::Bytes, provider: &config::ProviderConfig) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return body.to_vec();
    };

    if let Some(messages) = value.get_mut("messages").and_then(|m| m.as_array_mut()) {
        normalize_messages_for_upstream(messages);
    }

    let upstream = provider.upstream_model();
    if !upstream.trim().is_empty() {
        value["model"] = serde_json::Value::String(upstream.to_string());
    }

    serde_json::to_vec(&value).unwrap_or_else(|_| body.to_vec())
}
