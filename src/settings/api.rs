use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::{Html, IntoResponse, Json};
use serde::Deserialize;

use crate::codex;
use crate::config::{self, AppConfig, ProviderConfig};
use crate::provider;
use crate::provider::codex_chat_reasoning::{
    provider_supports_reasoning_effort_levels, supported_reasoning_levels_for_catalog,
};
use crate::provider::models;
use crate::provider::presets;
use crate::proxy::{reload_config_in_state, request_tray_health_check, ProxyState};
use crate::settings;

const SETTINGS_HTML: &str = include_str!("page.html");
const BRAND_ICON_SVG: &str = include_str!("../../assets/brand-icon.svg");

pub async fn settings_page() -> Html<String> {
    Html(SETTINGS_HTML.replace("<!--BRAND_ICON-->", BRAND_ICON_SVG.trim()))
}

pub async fn brand_icon_svg() -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/svg+xml; charset=utf-8"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400"),
    );
    (headers, BRAND_ICON_SVG)
}

fn mask_key_preview(key: &str) -> String {
    if key.len() <= 8 {
        return "***".into();
    }
    format!("{}...{}", &key[..4], &key[key.len() - 4..])
}

pub async fn settings_bootstrap() -> impl IntoResponse {
    let app = match AppConfig::load() {
        Ok(a) => a,
        Err(err) => {
            return Json(serde_json::json!({
                "error": err.to_string(),
                "providers": [],
                "active": "",
            }))
            .into_response();
        }
    };

    let mut providers = Vec::new();
    for preset in provider::list_presets(&app) {
        let key_preview = config::resolve_api_key(&preset.api_key_env)
            .ok()
            .map(|k| mask_key_preview(&k));
        let supports_reasoning_effort = provider_supports_reasoning_effort_levels(preset);
        providers.push(serde_json::json!({
            "id": preset.id,
            "name": preset.name,
            "signup_url": settings::signup_url(&preset.id),
            "key_configured": key_preview.is_some(),
            "key_preview": key_preview,
            "base_url": preset.base_url,
            "base_url_customized": preset.base_url_customized,
            "is_custom": preset.id == "custom",
            "custom_models_text": if preset.id == "custom" {
                models::custom_models_to_text(&preset.custom_models)
            } else {
                String::new()
            },
            "supports_reasoning_effort_levels": supports_reasoning_effort,
            "supported_reasoning_levels": supported_reasoning_levels_for_catalog(preset),
        }));
    }

    Json(serde_json::json!({
        "active": app.active,
        "model_reasoning_effort": app.normalized_model_reasoning_effort(),
        "providers": providers,
    }))
    .into_response()
}

#[derive(Deserialize)]
pub struct SettingsSaveBody {
    provider_id: String,
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    model_reasoning_effort: String,
    #[serde(default)]
    custom_models_text: String,
}

pub async fn settings_save(
    State(state): State<ProxyState>,
    Json(body): Json<SettingsSaveBody>,
) -> impl IntoResponse {
    match save_api_key(
        &state,
        &body.provider_id,
        body.api_key.trim(),
        body.base_url.trim(),
        body.model_reasoning_effort.trim(),
        body.custom_models_text.as_str(),
    )
    .await
    {
        Ok(message) => Json(serde_json::json!({
            "ok": true,
            "message": message,
        }))
        .into_response(),
        Err(err) => Json(serde_json::json!({
            "ok": false,
            "message": format!("{err:#}"),
        }))
        .into_response(),
    }
}

#[derive(Deserialize)]
pub struct SettingsTestBody {
    provider_id: String,
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    custom_models_text: String,
}

pub async fn settings_test(Json(body): Json<SettingsTestBody>) -> impl IntoResponse {
    let app = match AppConfig::load() {
        Ok(a) => a,
        Err(err) => {
            return Json(serde_json::json!({
                "ok": false,
                "message": err.to_string(),
            }))
            .into_response();
        }
    };

    let mut provider = match provider::get_preset(&app, &body.provider_id) {
        Ok(p) => p.clone(),
        Err(err) => {
            return Json(serde_json::json!({
                "ok": false,
                "message": err.to_string(),
            }))
            .into_response();
        }
    };

    if let Err(err) = apply_provider_base_url(&mut provider, body.base_url.trim()) {
        return Json(serde_json::json!({
            "ok": false,
            "message": format!("{err:#}"),
        }))
        .into_response();
    }

    if let Err(err) = apply_custom_models_from_text(&mut provider, body.custom_models_text.as_str()) {
        return Json(serde_json::json!({
            "ok": false,
            "message": format!("{err:#}"),
        }))
        .into_response();
    }

    let api_key = match resolve_key_for_request(&provider, body.api_key.trim()) {
        Ok(key) => key,
        Err(err) => {
            return Json(serde_json::json!({
                "ok": false,
                "message": format!("{err:#}"),
            }))
            .into_response();
        }
    };
    match test_api_key(&provider, &api_key).await {
        Ok(()) => Json(serde_json::json!({
            "ok": true,
            "message": format!("{} 连接成功！", provider.name),
        }))
        .into_response(),
        Err(err) => Json(serde_json::json!({
            "ok": false,
            "message": format!("{err:#}"),
        }))
        .into_response(),
    }
}

pub async fn settings_clear_all(State(state): State<ProxyState>) -> impl IntoResponse {
    match clear_all_settings(&state).await {
        Ok(message) => Json(serde_json::json!({
            "ok": true,
            "message": message,
        }))
        .into_response(),
        Err(err) => Json(serde_json::json!({
            "ok": false,
            "message": format!("{err:#}"),
        }))
        .into_response(),
    }
}

async fn clear_all_settings(state: &ProxyState) -> anyhow::Result<String> {
    let app = AppConfig::clear_all_settings()?;
    codex::inject_proxy_config(&app)?;
    reload_config_in_state(state).await?;
    state.request_log.clear().await;
    request_tray_health_check(state);
    Ok(
        "已清除所有 Helper 配置（API Key、厂商选择、Base URL、中转站模型）。请重新填写 Key 并重启 Codex Desktop。"
            .into(),
    )
}

async fn save_api_key(
    state: &ProxyState,
    provider_id: &str,
    api_key: &str,
    base_url: &str,
    model_reasoning_effort: &str,
    custom_models_text: &str,
) -> anyhow::Result<String> {
    let mut app = AppConfig::load()?;
    provider::get_preset(&app, provider_id)?;
    let provider_cfg = app
        .providers
        .get_mut(provider_id)
        .ok_or_else(|| anyhow::anyhow!("未知模型预设: {provider_id}"))?;

    apply_provider_base_url(provider_cfg, base_url)?;
    apply_custom_models_from_text(provider_cfg, custom_models_text)?;

    let provider = provider_cfg.clone();
    if provider_supports_reasoning_effort_levels(&provider)
        && !model_reasoning_effort.is_empty() {
            app.model_reasoning_effort =
                config::normalize_model_reasoning_effort(model_reasoning_effort);
        }
    if !api_key.is_empty() {
        config::save_env_value(&provider.api_key_env, api_key)?;
    } else if config::resolve_api_key(&provider.api_key_env).is_err() {
        anyhow::bail!("API Key 不能为空");
    }

    app.save()?;

    codex::inject_proxy_config(&app)?;
    reload_config_in_state(state).await?;
    request_tray_health_check(state);

    Ok(format!(
        "已保存 {} 配置。请完全退出并重新打开 Codex Desktop。",
        provider.name
    ))
}

fn builtin_base_url(provider_id: &str) -> String {
    presets::builtin_presets()
        .into_iter()
        .find(|preset| preset.id == provider_id)
        .map(|preset| preset.base_url)
        .unwrap_or_default()
}

fn apply_provider_base_url(
    provider: &mut ProviderConfig,
    base_url: &str,
) -> anyhow::Result<()> {
    let default_url = builtin_base_url(&provider.id);

    if provider.id == "custom" {
        if base_url.is_empty() && provider.base_url.trim().is_empty() {
            anyhow::bail!("请填写 Base URL");
        }
        if !base_url.is_empty() {
            provider.base_url = config::validate_base_url(base_url)?;
        }
        provider.base_url_customized = true;
        return Ok(());
    }

    if base_url.is_empty() {
        provider.base_url = default_url;
        provider.base_url_customized = false;
        return Ok(());
    }

    provider.base_url = config::validate_base_url(base_url)?;
    provider.base_url_customized = provider.base_url != default_url;
    Ok(())
}

fn apply_custom_models_from_text(
    provider: &mut ProviderConfig,
    custom_models_text: &str,
) -> anyhow::Result<()> {
    if provider.id != "custom" {
        return Ok(());
    }

    if custom_models_text.trim().is_empty() {
        provider.custom_models.clear();
        models::ensure_valid_model(provider);
        return Ok(());
    }

    let parsed = models::parse_custom_models_text(custom_models_text)?;
    if parsed.is_empty() {
        anyhow::bail!("请至少填写一个中转站模型");
    }
    provider.custom_models = parsed;
    models::ensure_valid_model(provider);
    Ok(())
}

fn resolve_key_for_request(
    provider: &ProviderConfig,
    api_key: &str,
) -> anyhow::Result<String> {
    if !api_key.is_empty() {
        return Ok(api_key.to_string());
    }
    config::resolve_api_key(&provider.api_key_env)
}

pub async fn test_api_key(provider: &ProviderConfig, api_key: &str) -> anyhow::Result<()> {
    if api_key.is_empty() {
        anyhow::bail!("API Key 不能为空");
    }
    if provider.id == "custom" && provider.base_url.trim().is_empty() {
        anyhow::bail!("请填写 Base URL");
    }
    if provider.id == "custom" && models::models_for_provider(provider).is_empty() {
        anyhow::bail!("请至少填写一个中转站模型");
    }

    let client = config::build_upstream_client(std::time::Duration::from_secs(30))?;

    let url = format!(
        "{}/chat/completions",
        provider.base_url.trim_end_matches('/')
    );

    let body = serde_json::json!({
        "model": provider.upstream_model(),
        "messages": [{"role": "user", "content": "ping"}],
        "max_tokens": 8
    });

    let resp = client
        .post(url)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await?;

    if resp.status().is_success() {
        return Ok(());
    }

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    anyhow::bail!("连接失败 ({status})：{text}")
}
