use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::config::{AppConfig, ProviderConfig};
use crate::provider::codex_chat_reasoning::supported_reasoning_levels_for_catalog;
use crate::provider::models::{self};
use crate::paths;

pub fn resolve_catalog_path() -> anyhow::Result<PathBuf> {
    paths::codex_catalog_path()
}

pub fn write_model_catalog(app: &AppConfig) -> anyhow::Result<()> {
    paths::ensure_helper_dirs()?;
    let codex_home = paths::codex_home_dir()?;
    std::fs::create_dir_all(&codex_home)?;

    let catalog = build_merged_catalog(app)?;
    let raw = serde_json::to_string_pretty(&catalog)?;

    let primary = paths::codex_catalog_path()?;
    crate::config::write_atomic(&primary, &raw)?;

    // 备份一份到 helper 目录，便于排查
    let backup = paths::helper_catalog_path()?;
    crate::config::write_atomic(&backup, &raw)?;
    Ok(())
}

pub fn catalog_path_string() -> anyhow::Result<String> {
    let path = resolve_catalog_path()?;
    Ok(path.display().to_string().replace('\\', "/"))
}

fn build_merged_catalog(app: &AppConfig) -> anyhow::Result<serde_json::Value> {
    let active = app.active_provider()?;
    let default_reasoning_effort = app.normalized_model_reasoning_effort();
    let active_variants = models::models_for_provider(active);
    let active_slugs: HashSet<String> = active_variants
        .iter()
        .map(|variant| variant.slug.clone())
        .collect();

    let template = load_catalog_template()?;
    let mut by_slug: HashMap<String, serde_json::Value> = HashMap::new();

    for existing in load_existing_catalog_models()? {
        if let Some(slug) = existing.get("slug").and_then(|v| v.as_str()) {
            if active_slugs.contains(slug) {
                by_slug.insert(slug.to_string(), existing);
            }
        }
    }

    for variant in &active_variants {
        let slug = variant.slug.clone();
        let is_active = active.default_model == variant.slug;
        let entry = if let Some(mut model) = by_slug.remove(&slug) {
            patch_variant_metadata(&mut model, active, variant, true);
            apply_reasoning_catalog_fields(&mut model, active, &default_reasoning_effort);
            if is_active {
                ensure_variant_display_name(&mut model, variant);
            }
            model
        } else {
            let mut model = model_from_variant(&template, active, variant);
            apply_reasoning_catalog_fields(&mut model, active, &default_reasoning_effort);
            if is_active {
                ensure_variant_display_name(&mut model, variant);
            }
            model
        };
        by_slug.insert(slug, entry);
    }

    let active_slug = active.catalog_model().to_string();
    let mut models: Vec<serde_json::Value> = by_slug.into_values().collect();
    for model in models.iter_mut() {
        ensure_required_v26_fields(model);
    }
    models.sort_by(|a, b| {
        let a_active = a.get("slug").and_then(|v| v.as_str()) == Some(active_slug.as_str());
        let b_active = b.get("slug").and_then(|v| v.as_str()) == Some(active_slug.as_str());
        if a_active != b_active {
            return b_active.cmp(&a_active);
        }
        let pa = a.get("priority").and_then(|v| v.as_i64()).unwrap_or(0);
        let pb = b.get("priority").and_then(|v| v.as_i64()).unwrap_or(0);
        pb.cmp(&pa)
    });

    Ok(serde_json::json!({ "models": models }))
}

/// Codex Desktop v26.527+ 用 `serde_json` 严格反序列化 catalog，缺任何 ModelInfo 必填字段
/// 都会直接报 "missing field `xxx`" 并拒绝启动。CC Switch 老模板和 helper 早期版本生成的
/// catalog 都缺这批字段，所以无论 entry 来源是模板还是 fallback，统一在写入前补一遍。
///
/// 必填集合摘自 codex-rs `ModelInfo` schema（issue #14757 评论里有完整表）：
fn ensure_required_v26_fields(model: &mut serde_json::Value) {
    let Some(obj) = model.as_object_mut() else {
        return;
    };
    let defaults: [(&str, serde_json::Value); 7] = [
        ("availability_nux", serde_json::Value::Null),
        ("upgrade", serde_json::Value::Null),
        (
            "base_instructions",
            serde_json::Value::String("You are a helpful coding agent.".into()),
        ),
        ("support_verbosity", serde_json::Value::Bool(false)),
        ("default_verbosity", serde_json::Value::Null),
        (
            "truncation_policy",
            serde_json::json!({ "mode": "tokens", "limit": 100_000 }),
        ),
        (
            "experimental_supported_tools",
            serde_json::Value::Array(vec![]),
        ),
    ];
    for (key, value) in defaults {
        if !obj.contains_key(key) {
            obj.insert(key.into(), value);
        }
    }
}

fn load_existing_catalog_models() -> anyhow::Result<Vec<serde_json::Value>> {
    let path = paths::cc_switch_catalog_path();
    if !path.exists() {
        return Ok(vec![]);
    }
    let raw = std::fs::read_to_string(&path)?;
    let catalog: serde_json::Value = serde_json::from_str(&raw)?;
    Ok(catalog
        .get("models")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default())
}

fn load_catalog_template() -> anyhow::Result<serde_json::Value> {
    for path in [paths::cc_switch_catalog_path()] {
        if !path.exists() {
            continue;
        }
        let raw = std::fs::read_to_string(&path)?;
        let catalog: serde_json::Value = serde_json::from_str(&raw)?;
        if let Some(template) = catalog
            .get("models")
            .and_then(|m| m.as_array())
            .and_then(|arr| arr.first())
        {
            return Ok(template.clone());
        }
    }

    Ok(minimal_model_entry(
        &ProviderConfig::new(
            "template",
            "Template",
            "",
            "",
            "deepseek-v4-flash",
            "",
            "chat",
        ),
    ))
}

fn model_from_variant(
    template: &serde_json::Value,
    provider: &ProviderConfig,
    variant: &models::ModelEntry,
) -> serde_json::Value {
    let mut model = template.clone();
    patch_variant_metadata(&mut model, provider, variant, false);
    apply_context_window(&mut model, variant.context_window);
    model
}

fn ensure_variant_display_name(model: &mut serde_json::Value, variant: &models::ModelEntry) {
    if let Some(obj) = model.as_object_mut() {
        obj.insert("display_name".into(), variant.display_name.clone().into());
    }
}

fn patch_variant_metadata(
    model: &mut serde_json::Value,
    provider: &ProviderConfig,
    variant: &models::ModelEntry,
    preserve_labels: bool,
) {
    if let Some(obj) = model.as_object_mut() {
        obj.insert("slug".into(), variant.slug.clone().into());
        if !preserve_labels {
            obj.insert("display_name".into(), variant.display_name.clone().into());
            obj.insert(
                "description".into(),
                format!("{} · {}", variant.display_name, provider.name).into(),
            );
        }
        let priority = if provider.id == "deepseek" && variant.slug == "deepseek-v4-pro" {
            1000
        } else if variant.context_window >= 1_000_000 {
            200
        } else {
            100
        };
        obj.insert("priority".into(), priority.into());
        obj.insert("visibility".into(), "list".into());
        obj.insert("supported_in_api".into(), true.into());
        apply_context_window(model, variant.context_window);
    }
}

fn apply_context_window(model: &mut serde_json::Value, context_window: u32) {
    if let Some(obj) = model.as_object_mut() {
        obj.insert("context_window".into(), context_window.into());
        obj.insert("max_context_window".into(), context_window.into());
    }
}

fn minimal_model_entry(provider: &ProviderConfig) -> serde_json::Value {
    let mut entry = serde_json::json!({
        "slug": provider.catalog_model(),
        "display_name": provider.catalog_display_name(),
        "description": format!("{} · Codex Helper", provider.name),
        "visibility": "list",
        "supported_in_api": true,
        "priority": provider.catalog_priority(),
        "context_window": 128000,
        "max_context_window": 128000,
        "effective_context_window_percent": 95,
        "input_modalities": ["text"],
        "supports_parallel_tool_calls": true,
        "supports_reasoning_summaries": false,
        "apply_patch_tool_type": "freeform",
        "shell_type": "shell_command",
        "web_search_tool_type": "text_and_image",
    });
    apply_reasoning_catalog_fields(&mut entry, provider, crate::config::DEFAULT_MODEL_REASONING_EFFORT);
    ensure_required_v26_fields(&mut entry);
    entry
}

fn apply_reasoning_catalog_fields(
    model: &mut serde_json::Value,
    provider: &ProviderConfig,
    default_reasoning_effort: &str,
) {
    let Some(obj) = model.as_object_mut() else {
        return;
    };

    if let Some(levels) = supported_reasoning_levels_for_catalog(provider) {
        obj.insert(
            "default_reasoning_level".into(),
            default_reasoning_effort.into(),
        );
        obj.insert("supported_reasoning_levels".into(), levels);
    } else {
        obj.remove("default_reasoning_level");
        obj.remove("supported_reasoning_levels");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    #[test]
    fn catalog_exposes_reasoning_levels_only_for_effort_providers() {
        let mut app = AppConfig::default();
        app.model_reasoning_effort = "high".into();

        app.active = "deepseek".into();
        let deepseek = build_merged_catalog(&app).unwrap();
        let deepseek_model = &deepseek["models"].as_array().unwrap()[0];
        assert!(deepseek_model.get("supported_reasoning_levels").is_some());
        assert_eq!(deepseek_model["default_reasoning_level"], "high");

        app.active = "kimi".into();
        let kimi = build_merged_catalog(&app).unwrap();
        let kimi_model = &kimi["models"].as_array().unwrap()[0];
        assert!(kimi_model.get("supported_reasoning_levels").is_none());
        assert!(kimi_model.get("default_reasoning_level").is_none());
    }

    #[test]
    fn catalog_lists_only_active_provider_models() {
        let app = AppConfig::default();
        let catalog = build_merged_catalog(&app).unwrap();
        let slugs: HashSet<String> = catalog["models"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| m.get("slug").and_then(|v| v.as_str()))
            .map(str::to_string)
            .collect();

        let expected: HashSet<String> = models::popular_models("deepseek")
            .iter()
            .map(|v| v.slug.to_string())
            .collect();
        assert_eq!(slugs, expected);

        assert!(!slugs.contains("qwen3.7-max"));
        assert!(!slugs.contains("glm-5"));
    }
}
