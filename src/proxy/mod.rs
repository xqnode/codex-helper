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
use tokio::sync::{Notify, RwLock};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

use crate::config::{self, AppConfig};

mod chat_to_responses;
mod codex_compat;
mod codex_tool_context;
mod logged_stream;
mod reasoning_options;
mod responses_failed;
mod responses_to_chat;
mod upstream_retry;
use logged_stream::LoggingByteStream;
use responses_to_chat::{
    convert_responses_to_chat_with_provider, finalize_chat_request, normalize_messages_for_upstream,
    repair_messages_for_upstream_with_options,
};
use crate::logs::{logs_bootstrap, logs_clear, logs_page};
use crate::request_log::{
    extract_model_from_body, parse_usage_from_json, PendingRequest, RequestLogStore,
};
use crate::settings::{brand_icon_svg, settings_bootstrap, settings_clear_all, settings_page, settings_save, settings_test};

type TrayHealthCheckHook = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
pub struct ProxyState {
    pub config: Arc<RwLock<AppConfig>>,
    /// 非流式上游请求（总超时见 `DEFAULT_UPSTREAM_REQUEST_TIMEOUT_SECS`）。
    pub client: Client,
    /// 流式上游请求（无总超时，读空闲超时见 `DEFAULT_UPSTREAM_STREAM_READ_IDLE_TIMEOUT_SECS`）。
    pub streaming_client: Client,
    pub request_log: RequestLogStore,
    tray_health_check: Arc<StdRwLock<Option<TrayHealthCheckHook>>>,
    shutdown: Arc<Notify>,
}

pub async fn spawn_server(config: AppConfig) -> anyhow::Result<Arc<ProxyState>> {
    config::validate_proxy_bind_host(&config.proxy.host)?;
    let (client, streaming_client) = config::build_proxy_upstream_clients()?;
    let shutdown = Arc::new(Notify::new());
    let state = Arc::new(ProxyState {
        config: Arc::new(RwLock::new(config.clone())),
        client,
        streaming_client,
        request_log: RequestLogStore::new(),
        tray_health_check: Arc::new(StdRwLock::new(None)),
        shutdown: shutdown.clone(),
    });
    let addr = format!("{}:{}", config.proxy.host, config.proxy.port);
    let serve_state = state.clone();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        if let Err(err) = run_listener(serve_state, &addr, Some(ready_tx)).await {
            tracing::error!("代理异常退出: {err:#}");
        }
    });
    match ready_rx.await {
        Ok(Ok(())) => Ok(state),
        Ok(Err(err)) => Err(err),
        Err(_) => anyhow::bail!("代理启动失败：监听任务在绑定端口前退出"),
    }
}

/// 托盘退出时优雅关闭 axum，让流式日志有机会落盘。
pub async fn shutdown(state: &ProxyState) {
    state.shutdown.notify_one();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
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
    config::validate_proxy_bind_host(&config.proxy.host)?;
    let addr = format!("{}:{}", config.proxy.host, config.proxy.port);
    let (client, streaming_client) = config::build_proxy_upstream_clients()?;
    let state = ProxyState {
        config: Arc::new(RwLock::new(config.clone())),
        client,
        streaming_client,
        request_log: RequestLogStore::new(),
        tray_health_check: Arc::new(StdRwLock::new(None)),
        shutdown: Arc::new(Notify::new()),
    };
    run_listener(Arc::new(state), &addr, None).await
}

async fn run_listener(
    state: Arc<ProxyState>,
    addr: &str,
    ready: Option<tokio::sync::oneshot::Sender<anyhow::Result<()>>>,
) -> anyhow::Result<()> {
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

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(e) => {
            let err = anyhow::anyhow!(
                "无法绑定 {addr}: {e}。端口 {} 可能被其他 codex-helper 实例占用。",
                config::DEFAULT_PORT
            );
            if let Some(tx) = ready {
                let _ = tx.send(Err(anyhow::anyhow!("{err:#}")));
            }
            return Err(err);
        }
    };

    if let Some(tx) = ready {
        let _ = tx.send(Ok(()));
    }

    let shutdown = state.shutdown.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown.notified().await;
        })
        .await?;
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
    forward_request(
        &state,
        "/chat/completions",
        Method::POST,
        headers,
        body,
        false,
    )
    .await
}

async fn proxy_responses(
    State(state): State<ProxyState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let provider = match state.config.read().await.active_provider() {
        Ok(provider) => provider.clone(),
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({ "error": err.to_string() })),
            )
                .into_response();
        }
    };
    if provider.uses_upstream_responses_api() {
        let patched = patch_upstream_model(&body, &provider);
        return forward_request(
            &state,
            "/responses",
            Method::POST,
            headers,
            patched.into(),
            true,
        )
        .await;
    }
    let tool_output_max_chars = state.config.read().await.tool_output_max_chars;
    match convert_responses_to_chat_with_provider(&body, Some(&provider), tool_output_max_chars) {
        Ok(converted) => forward_responses_request(&state, headers, converted).await,
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
    converted: responses_to_chat::ConvertedChatRequest,
) -> Response {
    let config = state.config.read().await.clone();
    let provider = match config.active_provider() {
        Ok(provider) => provider,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({ "error": err.to_string() })),
            )
                .into_response();
        }
    };
    let body = patch_upstream_model(&converted.body, provider);
    let response = forward_request(
        state,
        "/chat/completions",
        Method::POST,
        headers,
        body.into(),
        true,
    )
    .await;
    convert_chat_response_to_responses(
        response,
        converted.tool_context,
        converted.stream,
        &converted.model,
    )
    .await
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
    forward_request(&state, upstream_path, method, headers, body, false).await
}

fn extract_client_request_id(headers: &HeaderMap) -> String {
    for key in ["x-request-id", "x-correlation-id", "traceparent"] {
        if let Some(value) = headers.get(key) {
            if let Ok(text) = value.to_str() {
                let text = text.trim();
                if !text.is_empty() {
                    return text.to_string();
                }
            }
        }
    }
    String::new()
}

async fn forward_request(
    state: &ProxyState,
    upstream_path: &str,
    method: Method,
    headers: HeaderMap,
    body: axum::body::Bytes,
    already_normalized: bool,
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
    } else if already_normalized {
        Some(patch_upstream_model(&body, &provider))
    } else {
        Some(normalize_upstream_body(
            &body,
            &provider,
            config.tool_output_max_chars,
        ))
    };
    let stream_request = is_streaming_upstream_body(rewritten_body.as_deref());
    let model = rewritten_body
        .as_ref()
        .map(|bytes| extract_model_from_body(bytes, provider.upstream_model()))
        .unwrap_or_else(|| provider.upstream_model().to_string());
    let pending_base = PendingRequest {
        provider_id: provider.id.clone(),
        provider_name: provider.name.clone(),
        model,
        path: upstream_path.to_string(),
        client_request_id: extract_client_request_id(&headers),
        stream: stream_request,
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

    let upstream_client = if stream_request {
        &state.streaming_client
    } else {
        &state.client
    };

    for attempt in 0..upstream_retry::MAX_UPSTREAM_ATTEMPTS {
        let mut request = upstream_client.request(method.clone(), &target);
        request = request.header("Authorization", format!("Bearer {api_key}"));
        request = request.header("Content-Type", "application/json");
        // 不转发 Codex 客户端请求头（Accept / OpenAI-Beta 等），避免中转站报 Unsupported content type
        if let Some(rewritten) = rewritten_body.as_ref() {
            request = request.body(rewritten.clone());
        }

        match request.send().await {
            Ok(resp) => {
                if upstream_retry::is_retryable_upstream_status(resp.status())
                    && attempt + 1 < upstream_retry::MAX_UPSTREAM_ATTEMPTS
                {
                    let delay =
                        upstream_retry::retry_delay_from_headers(resp.headers(), attempt);
                    warn!(
                        "上游返回 {}，{:?} 后重试 ({}/{})",
                        resp.status(),
                        delay,
                        attempt + 2,
                        upstream_retry::MAX_UPSTREAM_ATTEMPTS
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }
                return finish_upstream_response(resp, pending_base, state).await;
            }
            Err(err) => {
                if upstream_retry::is_retryable_upstream_error(&err)
                    && attempt + 1 < upstream_retry::MAX_UPSTREAM_ATTEMPTS
                {
                    let delay = upstream_retry::retry_backoff(attempt);
                    warn!(
                        "上游连接失败，{:?} 后重试 ({}/{}): {target} -> {err}",
                        delay,
                        attempt + 2,
                        upstream_retry::MAX_UPSTREAM_ATTEMPTS
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }

                let timeout_hint = upstream_timeout_hint(stream_request, &err);
                warn!("上游请求失败: {target} -> {err}{timeout_hint}");
                let mut pending = pending_base;
                pending.status = StatusCode::BAD_GATEWAY.as_u16();
                let entry = state.request_log.finalize(pending, None);
                state.request_log.push(entry).await;
                return (
                    StatusCode::BAD_GATEWAY,
                    axum::Json(serde_json::json!({
                        "error": {
                            "message": format!("上游请求失败: {err}{timeout_hint}"),
                            "type": "upstream_error"
                        }
                    })),
                )
                    .into_response();
            }
        }
    }

    unreachable!("upstream retry loop must return inside");
}

async fn finish_upstream_response(
    resp: reqwest::Response,
    pending_base: PendingRequest,
    state: &ProxyState,
) -> Response {
    let status = resp.status();
    let mut pending = pending_base;
    pending.status = status.as_u16();
    let mut response_headers = HeaderMap::new();
    let mut is_sse = false;
    for (name, value) in resp.headers() {
        if name == reqwest::header::TRANSFER_ENCODING || name == reqwest::header::CONNECTION {
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

fn is_streaming_upstream_body(body: Option<&[u8]>) -> bool {
    let Some(bytes) = body else {
        return false;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return false;
    };
    value
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn upstream_timeout_hint(stream_request: bool, err: &reqwest::Error) -> &'static str {
    if !err.is_timeout() {
        return "";
    }
    if stream_request {
        return "（流式读空闲超时：上游超过 300 秒未发送新数据）";
    }
    "（非流式请求总超时：上游在 600 秒内未完成响应）"
}

async fn convert_chat_response_to_responses(
    response: Response,
    tool_context: codex_tool_context::CodexToolContext,
    stream_request: bool,
    model: &str,
) -> Response {
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if !status.is_success() {
        let body = response.into_body();
        let bytes = match axum::body::to_bytes(body, usize::MAX).await {
            Ok(bytes) => bytes,
            Err(err) => {
                warn!("读取上游错误响应失败: {err}");
                return responses_failed::responses_failed_http_response(
                    StatusCode::BAD_GATEWAY,
                    stream_request,
                    &format!("读取上游错误响应失败: {err}"),
                    Some("upstream_error"),
                    model,
                );
            }
        };
        let (message, error_type) = responses_failed::extract_upstream_error_message(&bytes);
        warn!("上游返回错误: status={status} message={message}");
        return responses_failed::responses_failed_http_response(
            status,
            stream_request,
            &message,
            error_type.as_deref(),
            model,
        );
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
        let translated =
            codex_compat::create_responses_sse_stream_from_chat_with_context(
                upstream_stream,
                tool_context,
            );
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
            return responses_failed::responses_failed_http_response(
                StatusCode::BAD_GATEWAY,
                stream_request,
                &format!("读取上游响应失败: {err}"),
                Some("upstream_error"),
                model,
            );
        }
    };

    match chat_json_to_responses_json(&bytes, &tool_context) {
        Ok(converted) => (
            status,
            [(reqwest::header::CONTENT_TYPE.as_str(), "application/json")],
            converted,
        )
            .into_response(),
        Err(err) => {
            let (message, error_type) = responses_failed::extract_upstream_error_message(&bytes);
            let message = if message.is_empty() {
                format!("Chat 响应转换 Responses 失败: {err}")
            } else {
                format!("Chat 响应转换 Responses 失败: {err}（上游返回: {message}）")
            };
            warn!("{message}");
            responses_failed::responses_failed_http_response(
                StatusCode::BAD_GATEWAY,
                stream_request,
                &message,
                error_type.as_deref().or(Some("upstream_error")),
                model,
            )
        }
    }
}

fn chat_json_to_responses_json(
    bytes: &[u8],
    tool_context: &codex_tool_context::CodexToolContext,
) -> anyhow::Result<String> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let response = chat_to_responses::chat_completion_to_response_with_context(&value, tool_context)?;
    Ok(serde_json::to_string(&response)?)
}

/// Responses→Chat 转换后的请求体只需替换上游 model，避免重复 normalize/repair。
fn patch_upstream_model(body: &[u8], provider: &config::ProviderConfig) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return body.to_vec();
    };

    let upstream = provider.upstream_model();
    if !upstream.trim().is_empty() {
        value["model"] = serde_json::Value::String(upstream.to_string());
    }

    serde_json::to_vec(&value).unwrap_or_else(|_| body.to_vec())
}

fn normalize_upstream_body(
    body: &axum::body::Bytes,
    provider: &config::ProviderConfig,
    tool_output_max_chars: usize,
) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return body.to_vec();
    };

    if let Some(messages) = value.get_mut("messages").and_then(|m| m.as_array_mut()) {
        normalize_messages_for_upstream(messages);
        repair_messages_for_upstream_with_options(
            messages,
            responses_to_chat::repair_options_for_provider(
                Some(provider),
                tool_output_max_chars,
            ),
        );
    }

    let stream = value
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    finalize_chat_request(&mut value, stream);
    reasoning_options::apply_reasoning_options(&mut value, provider);

    let upstream = provider.upstream_model();
    if !upstream.trim().is_empty() {
        value["model"] = serde_json::Value::String(upstream.to_string());
    }

    serde_json::to_vec(&value).unwrap_or_else(|_| body.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deepseek_provider() -> config::ProviderConfig {
        config::ProviderConfig::new(
            "deepseek",
            "DeepSeek",
            "https://api.deepseek.com/v1",
            "DEEPSEEK_API_KEY",
            "deepseek-v4-flash",
            "deepseek-v4-flash",
            "responses",
        )
    }

    fn qwen_provider() -> config::ProviderConfig {
        config::ProviderConfig::new(
            "qwen",
            "千问",
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
            "DASHSCOPE_API_KEY",
            "qwen3.7-max",
            "qwen3.7-max",
            "responses",
        )
    }

    #[test]
    fn maps_reasoning_effort_for_deepseek_provider() {
        let body = br#"{"model":"deepseek-v4-pro","stream":false,"reasoning":{"effort":"high"},"messages":[{"role":"user","content":"hi"}]}"#;
        let out = normalize_upstream_body(
            &axum::body::Bytes::from_static(body),
            &deepseek_provider(),
            0,
        );
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["reasoning_effort"], "high");
        assert_eq!(v["thinking"]["type"], "enabled");
        assert!(v.get("reasoning").is_none());
    }

    #[test]
    fn injects_reasoning_placeholder_only_for_thinking_providers() {
        let body = br#"{"model":"deepseek-v4-flash","messages":[
            {"role":"user","content":"run"},
            {"role":"assistant","content":"","tool_calls":[{"id":"call_1","type":"function","function":{"name":"a","arguments":"{}"}}]},
            {"role":"tool","tool_call_id":"call_1","content":"ok"}
        ]}"#;
        let deepseek_out = normalize_upstream_body(
            &axum::body::Bytes::from_static(body),
            &deepseek_provider(),
            0,
        );
        let qwen_out = normalize_upstream_body(
            &axum::body::Bytes::from_static(body),
            &qwen_provider(),
            0,
        );
        let deepseek: serde_json::Value = serde_json::from_slice(&deepseek_out).unwrap();
        let qwen: serde_json::Value = serde_json::from_slice(&qwen_out).unwrap();
        assert_eq!(deepseek["messages"][1]["reasoning_content"], "tool call");
        assert!(qwen["messages"][1].get("reasoning_content").is_none());
    }

    #[test]
    fn maps_developer_role_to_system() {
        let body = br#"{"model":"deepseek-v4-flash","messages":[{"role":"developer","content":[{"type":"input_text","text":"hi"}]}]}"#;
        let out = normalize_upstream_body(
            &axum::body::Bytes::from_static(body),
            &config::ProviderConfig::new(
                "deepseek",
                "DeepSeek",
                "https://api.deepseek.com/v1",
                "DEEPSEEK_API_KEY",
                "deepseek-v4-flash",
                "deepseek-v4-flash",
                "responses",
            ),
            0,
        );
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["messages"][0]["role"], "system");
        assert_eq!(v["messages"][0]["content"], "hi");
        assert_eq!(v["model"], "deepseek-v4-flash");
    }

    #[test]
    fn patch_upstream_model_only_replaces_model_field() {
        let provider = deepseek_provider();
        let body = br#"{"model":"gpt-5.4","stream":true,"messages":[{"role":"developer","content":"hi"}]}"#;
        let out = patch_upstream_model(body, &provider);
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["model"], "deepseek-v4-flash");
        assert_eq!(v["messages"][0]["role"], "developer");
    }

    #[test]
    fn detects_streaming_flag_in_upstream_body() {
        assert!(!is_streaming_upstream_body(None));
        assert!(!is_streaming_upstream_body(Some(br#"{"model":"m","stream":false}"#)));
        assert!(is_streaming_upstream_body(Some(br#"{"model":"m","stream":true}"#)));
    }

    #[test]
    fn wraps_chat_response_as_responses_json() {
        let body = br#"{"id":"chatcmpl_1","object":"chat.completion","created":123,"model":"deepseek-chat","choices":[{"message":{"role":"assistant","content":"pong"}}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
        let out =
            chat_json_to_responses_json(body, &codex_tool_context::CodexToolContext::default())
                .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["object"], "response");
        assert_eq!(v["status"], "completed");
        assert_eq!(v["output_text"], "pong");
        assert_eq!(v["output"][0]["content"][0]["type"], "output_text");
    }

    #[test]
    fn extract_client_request_id_reads_common_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-request-id", HeaderValue::from_static("req-abc"));
        assert_eq!(super::extract_client_request_id(&headers), "req-abc");
    }
}
