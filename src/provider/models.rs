#[derive(Debug, Clone, Copy)]
pub struct ModelVariant {
    pub slug: &'static str,
    pub display_name: &'static str,
    pub api_model: &'static str,
    pub context_window: u32,
}

pub fn popular_models(provider_id: &str) -> &'static [ModelVariant] {
    match provider_id {
        "deepseek" => &DEEPSEEK_MODELS,
        "qwen" => &QWEN_MODELS,
        "zhipu" => &ZHIPU_MODELS,
        "kimi" => &KIMI_MODELS,
        "minimax" => &MINIMAX_MODELS,
        _ => &[],
    }
}

pub fn find_model(provider_id: &str, slug: &str) -> Option<&'static ModelVariant> {
    popular_models(provider_id)
        .iter()
        .find(|m| m.slug == slug)
}

const DEEPSEEK_MODELS: &[ModelVariant] = &[
    ModelVariant {
        slug: "deepseek-v4-flash",
        display_name: "DeepSeek V4 Flash（推荐）",
        api_model: "deepseek-v4-flash",
        context_window: 1_000_000,
    },
    ModelVariant {
        slug: "deepseek-v4-pro",
        display_name: "DeepSeek V4 Pro 推理",
        api_model: "deepseek-v4-pro",
        context_window: 1_000_000,
    },
];

const QWEN_MODELS: &[ModelVariant] = &[
    ModelVariant {
        slug: "qwen3.7-max",
        display_name: "千问 3.7 Max（旗舰）",
        api_model: "qwen3.7-max",
        context_window: 256_000,
    },
    ModelVariant {
        slug: "qwen3.7-plus",
        display_name: "千问 3.7 Plus（推荐·1M）",
        api_model: "qwen3.7-plus",
        context_window: 1_000_000,
    },
];

const ZHIPU_MODELS: &[ModelVariant] = &[
    ModelVariant {
        slug: "glm-5",
        display_name: "GLM-5（旗舰）",
        api_model: "glm-5",
        context_window: 200_000,
    },
    ModelVariant {
        slug: "glm-4.7",
        display_name: "GLM-4.7",
        api_model: "glm-4.7",
        context_window: 200_000,
    },
];

const KIMI_MODELS: &[ModelVariant] = &[
    ModelVariant {
        slug: "kimi-k2.6",
        display_name: "Kimi K2.6（旗舰）",
        api_model: "kimi-k2.6",
        context_window: 256_000,
    },
];

const MINIMAX_MODELS: &[ModelVariant] = &[
    ModelVariant {
        slug: "minimax-m3",
        display_name: "MiniMax M3（旗舰·1M）",
        api_model: "MiniMax-M3",
        context_window: 1_000_000,
    },
];

pub fn apply_model_variant(
    provider: &mut crate::config::ProviderConfig,
    slug: &str,
) -> anyhow::Result<()> {
    let variant = find_model(&provider.id, slug).ok_or_else(|| {
        anyhow::anyhow!("未知模型: {slug}")
    })?;
    provider.default_model = variant.slug.to_string();
    provider.api_model = variant.api_model.to_string();
    Ok(())
}

fn migrate_legacy_model_slug(provider: &mut crate::config::ProviderConfig) {
    let new_slug = match (provider.id.as_str(), provider.default_model.as_str()) {
        ("deepseek", "deepseek-chat") => "deepseek-v4-flash",
        ("deepseek", "deepseek-reasoner") => "deepseek-v4-pro",
        ("qwen", "qwen-max") => "qwen3.7-max",
        ("qwen", "qwen-turbo" | "qwen-plus" | "qwen-long") => "qwen3.7-plus",
        ("zhipu", "glm-4-plus" | "glm-4-air" | "glm-4-long" | "glm-4-flash") => "glm-5",
        ("kimi", slug) if slug == "kimi-k2.5" || slug.starts_with("moonshot-v1") => "kimi-k2.6",
        ("minimax", "abab6.5s-chat" | "abab6.5g-chat" | "minimax-m2.7" | "minimax-m2.5") => {
            "minimax-m3"
        }
        _ => return,
    };
    provider.default_model = new_slug.to_string();
}

pub fn ensure_valid_model(provider: &mut crate::config::ProviderConfig) {
    migrate_legacy_model_slug(provider);
    if find_model(&provider.id, &provider.default_model).is_some() {
        return;
    }
    if let Some(first) = popular_models(&provider.id).first() {
        provider.default_model = first.slug.to_string();
        provider.api_model = first.api_model.to_string();
    }
}

pub fn sync_model_metadata(provider: &mut crate::config::ProviderConfig) {
    ensure_valid_model(provider);
    if let Some(variant) = find_model(&provider.id, &provider.default_model) {
        provider.api_model = variant.api_model.to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderConfig;

    fn provider(id: &str, model: &str) -> ProviderConfig {
        ProviderConfig {
            id: id.into(),
            name: id.into(),
            base_url: "https://example.com/v1".into(),
            api_key_env: "KEY".into(),
            default_model: model.into(),
            api_model: model.into(),
            wire_api: "responses".into(),
        }
    }

    #[test]
    fn each_provider_lists_only_core_models() {
        assert_eq!(popular_models("deepseek").len(), 2);
        assert_eq!(popular_models("qwen").len(), 2);
        assert_eq!(popular_models("zhipu").len(), 2);
        assert_eq!(popular_models("kimi").len(), 1);
        assert_eq!(popular_models("minimax").len(), 1);
    }

    #[test]
    fn migrates_deprecated_and_legacy_slugs() {
        let cases = [
            ("deepseek", "deepseek-chat", "deepseek-v4-flash"),
            ("deepseek", "deepseek-reasoner", "deepseek-v4-pro"),
            ("qwen", "qwen-plus", "qwen3.7-plus"),
            ("zhipu", "glm-4-flash", "glm-5"),
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

