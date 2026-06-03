use crate::config::ProviderConfig;

pub fn builtin_presets() -> Vec<ProviderConfig> {
    vec![
        ProviderConfig {
            id: "deepseek".into(),
            name: "DeepSeek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            api_key_env: "DEEPSEEK_API_KEY".into(),
            default_model: "deepseek-v4-flash".into(),
            api_model: "deepseek-v4-flash".into(),
            wire_api: "responses".into(),
        },
        ProviderConfig {
            id: "qwen".into(),
            name: "千问".into(),
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
            api_key_env: "DASHSCOPE_API_KEY".into(),
            default_model: "qwen3.7-plus".into(),
            api_model: "qwen3.7-plus".into(),
            wire_api: "responses".into(),
        },
        ProviderConfig {
            id: "zhipu".into(),
            name: "智谱".into(),
            base_url: "https://open.bigmodel.cn/api/paas/v4".into(),
            api_key_env: "ZHIPU_API_KEY".into(),
            default_model: "glm-5".into(),
            api_model: "glm-5".into(),
            wire_api: "responses".into(),
        },
        ProviderConfig {
            id: "kimi".into(),
            name: "Kimi".into(),
            base_url: "https://api.moonshot.cn/v1".into(),
            api_key_env: "MOONSHOT_API_KEY".into(),
            default_model: "kimi-k2.6".into(),
            api_model: "kimi-k2.6".into(),
            wire_api: "responses".into(),
        },
        ProviderConfig {
            id: "minimax".into(),
            name: "Minimax".into(),
            base_url: "https://api.minimaxi.com/v1".into(),
            api_key_env: "MINIMAX_API_KEY".into(),
            default_model: "minimax-m3".into(),
            api_model: "MiniMax-M3".into(),
            wire_api: "responses".into(),
        },
    ]
}
