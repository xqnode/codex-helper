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
    ModelVariant {
        slug: "deepseek-chat",
        display_name: "DeepSeek Chat（兼容，2026/07 下线）",
        api_model: "deepseek-chat",
        context_window: 1_000_000,
    },
    ModelVariant {
        slug: "deepseek-reasoner",
        display_name: "DeepSeek Reasoner（兼容，2026/07 下线）",
        api_model: "deepseek-reasoner",
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
    ModelVariant {
        slug: "qwen-plus",
        display_name: "千问 Plus",
        api_model: "qwen-plus",
        context_window: 128_000,
    },
    ModelVariant {
        slug: "qwen-long",
        display_name: "千问 Long（1M 上下文）",
        api_model: "qwen-long",
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
    ModelVariant {
        slug: "glm-4-long",
        display_name: "GLM-4 Long（1M 上下文）",
        api_model: "glm-4-long",
        context_window: 1_000_000,
    },
    ModelVariant {
        slug: "glm-4-flash",
        display_name: "GLM-4 Flash",
        api_model: "glm-4-flash",
        context_window: 128_000,
    },
];

const KIMI_MODELS: &[ModelVariant] = &[
    ModelVariant {
        slug: "kimi-k2.6",
        display_name: "Kimi K2.6（旗舰）",
        api_model: "kimi-k2.6",
        context_window: 256_000,
    },
    ModelVariant {
        slug: "kimi-k2.5",
        display_name: "Kimi K2.5",
        api_model: "kimi-k2.5",
        context_window: 256_000,
    },
    ModelVariant {
        slug: "moonshot-v1-128k",
        display_name: "Moonshot V1 128K（旧版）",
        api_model: "moonshot-v1-128k",
        context_window: 128_000,
    },
    ModelVariant {
        slug: "moonshot-v1-32k",
        display_name: "Moonshot V1 32K（旧版）",
        api_model: "moonshot-v1-32k",
        context_window: 32_768,
    },
];

const MINIMAX_MODELS: &[ModelVariant] = &[
    ModelVariant {
        slug: "minimax-m3",
        display_name: "MiniMax M3（旗舰·1M）",
        api_model: "MiniMax-M3",
        context_window: 1_000_000,
    },
    ModelVariant {
        slug: "minimax-m2.7",
        display_name: "MiniMax M2.7",
        api_model: "MiniMax-M2.7",
        context_window: 204_800,
    },
    ModelVariant {
        slug: "minimax-m2.5",
        display_name: "MiniMax M2.5",
        api_model: "MiniMax-M2.5",
        context_window: 204_800,
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
        ("qwen", "qwen-max" | "qwen-turbo") => "qwen3.7-max",
        ("zhipu", "glm-4-plus" | "glm-4-air") => "glm-5",
        ("kimi", slug) if slug.starts_with("moonshot-v1") => "kimi-k2.6",
        ("minimax", "abab6.5s-chat" | "abab6.5g-chat") => "minimax-m3",
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
