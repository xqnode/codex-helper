#[derive(Debug, Clone)]
pub struct ModelEntry {
    pub slug: String,
    pub display_name: String,
    pub api_model: String,
    pub context_window: u32,
    pub menu_tag: String,
}

#[derive(Debug, Clone, Copy)]
pub struct ModelVariant {
    pub slug: &'static str,
    pub display_name: &'static str,
    pub api_model: &'static str,
    pub context_window: u32,
    /// 托盘菜单简称，如 flash / pro
    pub menu_tag: &'static str,
}

pub fn popular_models(provider_id: &str) -> &'static [ModelVariant] {
    match provider_id {
        "deepseek" => &DEEPSEEK_MODELS,
        "qwen" => &QWEN_MODELS,
        "zhipu" => &ZHIPU_MODELS,
        "kimi" => &KIMI_MODELS,
        "minimax" => &MINIMAX_MODELS,
        "mimo" => &MIMO_MODELS,
        "custom" => &OPENAI_MODELS,
        _ => &[],
    }
}

pub fn models_for_provider(provider: &crate::config::ProviderConfig) -> Vec<ModelEntry> {
    if provider.id == "custom" && !provider.custom_models.is_empty() {
        return provider
            .custom_models
            .iter()
            .map(custom_model_to_entry)
            .collect();
    }
    popular_models(&provider.id)
        .iter()
        .map(variant_to_entry)
        .collect()
}

pub fn find_model_entry(
    provider: &crate::config::ProviderConfig,
    slug: &str,
) -> Option<ModelEntry> {
    models_for_provider(provider)
        .into_iter()
        .find(|m| m.slug == slug)
}

pub fn find_model(provider_id: &str, slug: &str) -> Option<&'static ModelVariant> {
    popular_models(provider_id)
        .iter()
        .find(|m| m.slug == slug)
}

/// 托盘菜单用的型号简称（如 flash、pro）。
pub fn menu_tag(provider: &crate::config::ProviderConfig) -> Option<String> {
    find_model_entry(provider, &provider.default_model).map(|m| m.menu_tag)
}

/// 托盘菜单用，如 1M、256K。
pub fn format_context_window(tokens: u32) -> String {
    if tokens >= 1_000_000 && tokens % 1_000_000 == 0 {
        format!("{}M", tokens / 1_000_000)
    } else if tokens >= 1_000 && tokens % 1_000 == 0 {
        format!("{}K", tokens / 1_000)
    } else {
        tokens.to_string()
    }
}

pub fn tray_model_label(model: &ModelEntry, active: bool) -> String {
    let label = format!(
        "{} · {}",
        model.display_name,
        format_context_window(model.context_window)
    );
    if active {
        format!("✓ {label}")
    } else {
        label
    }
}

fn variant_to_entry(variant: &ModelVariant) -> ModelEntry {
    ModelEntry {
        slug: variant.slug.to_string(),
        display_name: variant.display_name.to_string(),
        api_model: variant.api_model.to_string(),
        context_window: variant.context_window,
        menu_tag: variant.menu_tag.to_string(),
    }
}

fn custom_model_to_entry(entry: &crate::config::CustomModelEntry) -> ModelEntry {
    let display_name = if entry.display_name.trim().is_empty() {
        entry.slug.clone()
    } else {
        entry.display_name.clone()
    };
    let menu_tag = entry
        .slug
        .split('-')
        .next_back()
        .unwrap_or(entry.slug.as_str())
        .to_string();
    ModelEntry {
        slug: entry.slug.clone(),
        display_name,
        api_model: entry.api_model.clone(),
        context_window: entry.context_window,
        menu_tag,
    }
}

pub fn custom_models_to_text(entries: &[crate::config::CustomModelEntry]) -> String {
    entries
        .iter()
        .map(|entry| {
            let display = if entry.display_name.trim().is_empty() {
                entry.slug.as_str()
            } else {
                entry.display_name.as_str()
            };
            if entry.api_model != entry.slug {
                if display != entry.slug.as_str() {
                    format!("{}|{}|{}", display, entry.slug, entry.api_model)
                } else {
                    format!("{}|{}", entry.slug, entry.api_model)
                }
            } else if display != entry.slug.as_str() {
                format!("{}|{}", display, entry.slug)
            } else {
                entry.slug.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn parse_custom_models_text(text: &str) -> anyhow::Result<Vec<crate::config::CustomModelEntry>> {
    let mut models = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let entry = parse_custom_model_line(line).ok_or_else(|| {
            anyhow::anyhow!("第 {} 行格式无效: {line}", index + 1)
        })?;
        if models.iter().any(|m: &crate::config::CustomModelEntry| m.slug == entry.slug) {
            anyhow::bail!("模型 slug 重复: {}", entry.slug);
        }
        models.push(entry);
    }
    Ok(models)
}

fn parse_custom_model_line(line: &str) -> Option<crate::config::CustomModelEntry> {
    let parts: Vec<&str> = line
        .split('|')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    let entry = match parts.len() {
        1 => {
            let slug = normalize_model_slug(parts[0])?;
            crate::config::CustomModelEntry {
                slug: slug.clone(),
                api_model: slug.clone(),
                display_name: slug,
                context_window: default_custom_context_window(),
            }
        }
        2 => {
            let slug = normalize_model_slug(parts[0])?;
            crate::config::CustomModelEntry {
                slug: slug.clone(),
                api_model: parts[1].to_string(),
                display_name: slug,
                context_window: default_custom_context_window(),
            }
        }
        3 => {
            let slug = normalize_model_slug(parts[1])?;
            crate::config::CustomModelEntry {
                display_name: parts[0].to_string(),
                slug: slug.clone(),
                api_model: parts[2].to_string(),
                context_window: default_custom_context_window(),
            }
        }
        _ => return None,
    };
    Some(entry)
}

fn normalize_model_slug(raw: &str) -> Option<String> {
    let slug = raw.trim();
    if slug.is_empty() {
        return None;
    }
    if !slug
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return None;
    }
    Some(slug.to_string())
}

fn default_custom_context_window() -> u32 {
    128_000
}

const DEEPSEEK_MODELS: &[ModelVariant] = &[
    ModelVariant {
        slug: "deepseek-v4-pro",
        display_name: "DeepSeek V4 Pro（旗舰）",
        api_model: "deepseek-v4-pro",
        context_window: 1_000_000,
        menu_tag: "pro",
    },
    ModelVariant {
        slug: "deepseek-v4-flash",
        display_name: "DeepSeek V4 Flash",
        api_model: "deepseek-v4-flash",
        context_window: 1_000_000,
        menu_tag: "flash",
    },
];

const QWEN_MODELS: &[ModelVariant] = &[
    ModelVariant {
        slug: "qwen3.7-max",
        display_name: "千问 3.7 Max（旗舰）",
        api_model: "qwen3.7-max",
        context_window: 1_000_000,
        menu_tag: "max",
    },
    ModelVariant {
        slug: "qwen3.7-plus",
        display_name: "千问 3.7 Plus（多模态·1M）",
        api_model: "qwen3.7-plus",
        context_window: 1_000_000,
        menu_tag: "plus",
    },
];

const ZHIPU_MODELS: &[ModelVariant] = &[
    ModelVariant {
        slug: "glm-5.1",
        display_name: "GLM-5.1（旗舰）",
        api_model: "glm-5.1",
        context_window: 200_000,
        menu_tag: "5.1",
    },
    ModelVariant {
        slug: "glm-5",
        display_name: "GLM-5",
        api_model: "glm-5",
        context_window: 200_000,
        menu_tag: "glm-5",
    },
    ModelVariant {
        slug: "glm-4.7",
        display_name: "GLM-4.7",
        api_model: "glm-4.7",
        context_window: 200_000,
        menu_tag: "4.7",
    },
];

const KIMI_MODELS: &[ModelVariant] = &[
    ModelVariant {
        slug: "kimi-k2.6",
        display_name: "Kimi K2.6（旗舰）",
        api_model: "kimi-k2.6",
        context_window: 256_000,
        menu_tag: "k2.6",
    },
];

const MINIMAX_MODELS: &[ModelVariant] = &[
    ModelVariant {
        slug: "minimax-m3",
        display_name: "MiniMax M3（旗舰·1M）",
        api_model: "MiniMax-M3",
        context_window: 1_000_000,
        menu_tag: "m3",
    },
];

const MIMO_MODELS: &[ModelVariant] = &[
    ModelVariant {
        slug: "mimo-v2.5-pro",
        display_name: "MiMo V2.5 Pro（旗舰·1M）",
        api_model: "mimo-v2.5-pro",
        context_window: 1_000_000,
        menu_tag: "pro",
    },
    ModelVariant {
        slug: "mimo-v2.5",
        display_name: "MiMo V2.5（全模态·1M）",
        api_model: "mimo-v2.5",
        context_window: 1_000_000,
        menu_tag: "2.5",
    },
    ModelVariant {
        slug: "mimo-v2-flash",
        display_name: "MiMo V2 Flash（256K）",
        api_model: "mimo-v2-flash",
        context_window: 256_000,
        menu_tag: "flash",
    },
];

const OPENAI_MODELS: &[ModelVariant] = &[
    ModelVariant {
        slug: "gpt-5.5",
        display_name: "GPT-5.5（推荐）",
        api_model: "gpt-5.5",
        context_window: 256_000,
        menu_tag: "5.5",
    },
    ModelVariant {
        slug: "gpt-5.4",
        display_name: "GPT-5.4",
        api_model: "gpt-5.4",
        context_window: 256_000,
        menu_tag: "5.4",
    },
    ModelVariant {
        slug: "gpt-5.4-mini",
        display_name: "GPT-5.4 Mini",
        api_model: "gpt-5.4-mini",
        context_window: 128_000,
        menu_tag: "4-mini",
    },
];

pub fn apply_model_variant(
    provider: &mut crate::config::ProviderConfig,
    slug: &str,
) -> anyhow::Result<()> {
    let entry = find_model_entry(provider, slug).ok_or_else(|| {
        anyhow::anyhow!("未知模型: {slug}")
    })?;
    provider.default_model = entry.slug;
    provider.api_model = entry.api_model;
    Ok(())
}

fn migrate_legacy_model_slug(provider: &mut crate::config::ProviderConfig) {
    let new_slug = match (provider.id.as_str(), provider.default_model.as_str()) {
        ("deepseek", "deepseek-chat") => "deepseek-v4-flash",
        ("deepseek", "deepseek-reasoner") => "deepseek-v4-pro",
        ("qwen", "qwen-max") => "qwen3.7-max",
        ("qwen", "qwen-turbo" | "qwen-plus" | "qwen-long") => "qwen3.7-plus",
        ("zhipu", "glm-4-plus" | "glm-4-air" | "glm-4-long" | "glm-4-flash") => "glm-5.1",
        ("kimi", slug) if slug == "kimi-k2.5" || slug.starts_with("moonshot-v1") => "kimi-k2.6",
        ("minimax", "abab6.5s-chat" | "abab6.5g-chat" | "minimax-m2.7" | "minimax-m2.5") => {
            "minimax-m3"
        }
        ("mimo", "mimo-v2-pro") => "mimo-v2.5-pro",
        ("mimo", "mimo-v2-omni") => "mimo-v2.5",
        ("mimo", slug) if slug.starts_with("mimo-v1") => "mimo-v2-flash",
        ("custom", "gpt-4o") => "gpt-5.5",
        _ => return,
    };
    provider.default_model = new_slug.to_string();
}

pub fn ensure_valid_model(provider: &mut crate::config::ProviderConfig) {
    migrate_legacy_model_slug(provider);
    if find_model_entry(provider, &provider.default_model).is_some() {
        if let Some(entry) = find_model_entry(provider, &provider.default_model) {
            provider.api_model = entry.api_model;
        }
        return;
    }
    if let Some(first) = models_for_provider(provider).first() {
        provider.default_model = first.slug.clone();
        provider.api_model = first.api_model.clone();
    }
}

pub fn sync_model_metadata(provider: &mut crate::config::ProviderConfig) {
    ensure_valid_model(provider);
    if provider.id == "custom" && !provider.custom_models.is_empty() {
        return;
    }
    if let Some(variant) = find_model(&provider.id, &provider.default_model) {
        provider.api_model = variant.api_model.to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderConfig;

    fn provider(id: &str, model: &str) -> ProviderConfig {
        ProviderConfig::new(id, id, "https://example.com/v1", "KEY", model, model, "responses")
    }

    #[test]
    fn each_provider_lists_only_core_models() {
        assert_eq!(popular_models("deepseek").len(), 2);
        assert_eq!(popular_models("qwen").len(), 2);
        assert_eq!(popular_models("zhipu").len(), 3);
        assert_eq!(popular_models("kimi").len(), 1);
        assert_eq!(popular_models("minimax").len(), 1);
        assert_eq!(popular_models("mimo").len(), 3);
        assert_eq!(popular_models("custom").len(), 3);
    }

    #[test]
    fn menu_tags_are_defined_for_core_models() {
        assert_eq!(find_model("deepseek", "deepseek-v4-pro").unwrap().menu_tag, "pro");
        assert_eq!(find_model("qwen", "qwen3.7-plus").unwrap().menu_tag, "plus");
    }

    #[test]
    fn tray_model_label_includes_context() {
        let model = variant_to_entry(find_model("deepseek", "deepseek-v4-flash").unwrap());
        assert_eq!(
            tray_model_label(&model, true),
            "✓ DeepSeek V4 Flash · 1M"
        );
        let glm = variant_to_entry(find_model("zhipu", "glm-5.1").unwrap());
        assert_eq!(tray_model_label(&glm, false), "GLM-5.1（旗舰） · 200K");
    }

    #[test]
    fn parse_custom_models_text_supports_multiple_formats() {
        let models = parse_custom_models_text(
            "gpt-5.5\ngpt-4o|gpt-4o-2024-08-06\nGPT-5.4|gpt-5.4|gpt-5.4",
        )
        .unwrap();
        assert_eq!(models.len(), 3);
        assert_eq!(models[0].slug, "gpt-5.5");
        assert_eq!(models[1].api_model, "gpt-4o-2024-08-06");
        assert_eq!(models[2].display_name, "GPT-5.4");
    }

    #[test]
    fn models_for_provider_uses_custom_models_when_present() {
        let mut provider = provider("custom", "gpt-5.5");
        provider.custom_models = vec![crate::config::CustomModelEntry {
            slug: "my-model".into(),
            api_model: "upstream-id".into(),
            display_name: "My Model".into(),
            context_window: 256_000,
        }];
        let models = models_for_provider(&provider);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].api_model, "upstream-id");
    }

    #[test]
    fn format_context_window_labels() {
        assert_eq!(format_context_window(1_000_000), "1M");
        assert_eq!(format_context_window(256_000), "256K");
        assert_eq!(format_context_window(128_000), "128K");
    }

    #[test]
    fn migrates_deprecated_and_legacy_slugs() {
        let cases = [
            ("deepseek", "deepseek-chat", "deepseek-v4-flash"),
            ("deepseek", "deepseek-reasoner", "deepseek-v4-pro"),
            ("qwen", "qwen-plus", "qwen3.7-plus"),
            ("zhipu", "glm-4-flash", "glm-5.1"),
            ("kimi", "moonshot-v1-128k", "kimi-k2.6"),
            ("minimax", "minimax-m2.5", "minimax-m3"),
        ];

        for (id, old, expected) in cases {
            let mut p = provider(id, old);
            ensure_valid_model(&mut p);
            assert_eq!(p.default_model, expected, "{id}/{old}");
        }
    }
}

