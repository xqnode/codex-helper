use axum::extract::State;
use axum::response::{Html, IntoResponse, Json};
use reqwest::Client;
use serde::Deserialize;

use crate::codex;
use crate::config::{self, AppConfig};
use crate::provider;
use crate::proxy::{reload_config_in_state, ProxyState};
use crate::settings;

const SETTINGS_HTML: &str = include_str!("page.html");

fn mask_key_preview(key: &str) -> String {
    if key.len() <= 8 {
        return "***".into();
    }
    format!("{}...{}", &key[..4], &key[key.len() - 4..])
}

fn format_context(tokens: u32) -> String {
    if tokens >= 1_000_000 {
        "1M".into()
    } else if tokens >= 1000 {
        format!("{}K", tokens / 1000)
    } else {
        tokens.to_string()
    }
}

pub async fn settings_page() -> Html<&'static str> {
    Html(SETTINGS_HTML)
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
        let key_configured = key_preview.is_some();
        providers.push(serde_json::json!({
            "id": preset.id,
            "name": preset.name,
            "env_key": preset.api_key_env,
            "signup_url": settings::signup_url(&preset.id),
            "key_configured": key_configured,
            "key_preview": key_preview,
            "current_model": preset.default_model,
            "models": provider::models::popular_models(&preset.id)
                .iter()
                .map(|m| serde_json::json!({
                    "slug": m.slug,
                    "name": m.display_name,
                    "context": format_context(m.context_window),
                }))
                .collect::<Vec<_>>(),
        }));
    }

    Json(serde_json::json!({
        "active": app.active,
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
    model_slug: String,
}

pub async fn settings_save(
    State(state): State<ProxyState>,
    Json(body): Json<SettingsSaveBody>,
) -> impl IntoResponse {
    match save_api_key(
        &state,
        &body.provider_id,
        body.api_key.trim(),
        body.model_slug.trim(),
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
    model_slug: String,
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

    if !body.model_slug.trim().is_empty() {
        let _ = provider::models::apply_model_variant(&mut provider, body.model_slug.trim());
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

async fn save_api_key(
    state: &ProxyState,
    provider_id: &str,
    api_key: &str,
    model_slug: &str,
) -> anyhow::Result<String> {
    let mut app = AppConfig::load()?;
    let provider_cfg = app
        .providers
        .get_mut(provider_id)
        .ok_or_else(|| anyhow::anyhow!("未知模型预设: {provider_id}"))?;

    if !model_slug.is_empty() {
        provider::models::apply_model_variant(provider_cfg, model_slug)?;
    }

    let provider = provider_cfg.clone();
    if !api_key.is_empty() {
        config::save_env_value(&provider.api_key_env, api_key)?;
    } else if config::resolve_api_key(&provider.api_key_env).is_err() {
        anyhow::bail!("API Key 不能为空");
    }

    if app.active != provider_id {
        app.active = provider_id.to_string();
    }
    app.save()?;

    codex::inject_proxy_config(&app)?;
    reload_config_in_state(state).await?;

    let model_label = provider::models::find_model(provider_id, &provider.default_model)
        .map(|m| m.display_name.to_string())
        .unwrap_or_else(|| provider.default_model.clone());

    Ok(format!(
        "已保存 {} · {}。请完全退出并重新打开 Codex Desktop。",
        provider.name, model_label
    ))
}

fn resolve_key_for_request(
    provider: &config::ProviderConfig,
    api_key: &str,
) -> anyhow::Result<String> {
    if !api_key.is_empty() {
        return Ok(api_key.to_string());
    }
    config::resolve_api_key(&provider.api_key_env)
}

pub async fn test_api_key(provider: &config::ProviderConfig, api_key: &str) -> anyhow::Result<()> {
    if api_key.is_empty() {
        anyhow::bail!("API Key 不能为空");
    }

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

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
