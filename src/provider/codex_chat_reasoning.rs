//! Codex Responses → Chat Completions 的 reasoning 能力描述（对齐 cc-switch）。

use serde_json::{json, Value};

use crate::config::ProviderConfig;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodexChatReasoningConfig {
    pub supports_thinking: Option<bool>,
    pub supports_effort: Option<bool>,
    pub thinking_param: Option<String>,
    pub effort_param: Option<String>,
    pub effort_value_mode: Option<String>,
    pub output_format: Option<String>,
    /// 多轮 tool 历史是否需补 `reasoning_content` 占位（DeepSeek/Kimi 等；千问不需要）。
    pub preserve_tool_call_reasoning: Option<bool>,
    /// `thinking.type` 在开启推理时的取值（默认 `enabled`；MiniMax M3 需 `adaptive`）。
    pub thinking_type_when_enabled: Option<String>,
    /// 开启推理时是否注入 `reasoning_split`（MiniMax M3 推荐开启）。
    pub reasoning_split_when_enabled: Option<bool>,
    /// `thinking.keep`（Kimi 多轮 tool 历史推荐 `all`）。
    pub thinking_keep_when_enabled: Option<String>,
    /// `thinking.clear_thinking`（智谱 GLM 跨轮保留推理时设为 `false`）。
    pub thinking_clear_thinking_when_enabled: Option<bool>,
}

impl ProviderConfig {
    pub fn codex_chat_reasoning_config(&self) -> Option<CodexChatReasoningConfig> {
        codex_chat_reasoning_config_for(self)
    }
}

pub fn codex_chat_reasoning_config_for(provider: &ProviderConfig) -> Option<CodexChatReasoningConfig> {
    match provider.id.as_str() {
        "deepseek" => Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(true),
            thinking_param: Some("thinking".into()),
            effort_param: Some("reasoning_effort".into()),
            effort_value_mode: Some("deepseek".into()),
            output_format: Some("reasoning_content".into()),
            preserve_tool_call_reasoning: Some(true),
            ..Default::default()
        }),
        "kimi" => Some(kimi_thinking()),
        "qwen" => Some(thinking_only("enable_thinking", false)),
        "minimax" => Some(minimax_thinking()),
        "mimo" => Some(thinking_only("enable_thinking", true)),
        "zhipu" => Some(zhipu_thinking()),
        "custom" => infer_custom_reasoning_config(&provider.base_url),
        _ => None,
    }
}

fn kimi_thinking() -> CodexChatReasoningConfig {
    CodexChatReasoningConfig {
        supports_thinking: Some(true),
        supports_effort: Some(false),
        thinking_param: Some("thinking".into()),
        effort_param: Some("none".into()),
        output_format: Some("reasoning_content".into()),
        preserve_tool_call_reasoning: Some(true),
        thinking_keep_when_enabled: Some("all".into()),
        ..Default::default()
    }
}

fn zhipu_thinking() -> CodexChatReasoningConfig {
    CodexChatReasoningConfig {
        supports_thinking: Some(true),
        supports_effort: Some(false),
        thinking_param: Some("thinking".into()),
        effort_param: Some("none".into()),
        output_format: Some("reasoning_content".into()),
        preserve_tool_call_reasoning: Some(false),
        ..Default::default()
    }
}

fn minimax_thinking() -> CodexChatReasoningConfig {
    CodexChatReasoningConfig {
        supports_thinking: Some(true),
        supports_effort: Some(false),
        thinking_param: Some("thinking".into()),
        effort_param: Some("none".into()),
        output_format: Some("reasoning_content".into()),
        preserve_tool_call_reasoning: Some(true),
        thinking_type_when_enabled: Some("adaptive".into()),
        reasoning_split_when_enabled: Some(true),
        ..Default::default()
    }
}

fn thinking_only(thinking_param: &str, preserve_tool_call_reasoning: bool) -> CodexChatReasoningConfig {
    CodexChatReasoningConfig {
        supports_thinking: Some(true),
        supports_effort: Some(false),
        thinking_param: Some(thinking_param.into()),
        effort_param: Some("none".into()),
        output_format: Some("reasoning_content".into()),
        preserve_tool_call_reasoning: Some(preserve_tool_call_reasoning),
        ..Default::default()
    }
}

/// 厂商是否支持 Codex 推理档位（`reasoning.effort` 会映射为不同上游强度）。
pub fn provider_supports_reasoning_effort_levels(provider: &ProviderConfig) -> bool {
    codex_chat_reasoning_config_for(provider)
        .map(|config| config.supports_effort.unwrap_or(false))
        .unwrap_or(false)
}

/// 写入 model catalog 的 `supported_reasoning_levels`；不支持分档时返回 `None`。
pub fn supported_reasoning_levels_for_catalog(provider: &ProviderConfig) -> Option<Value> {
    if !provider_supports_reasoning_effort_levels(provider) {
        return None;
    }

    let mode = codex_chat_reasoning_config_for(provider)
        .and_then(|config| config.effort_value_mode)
        .unwrap_or_default();

    Some(match mode.as_str() {
        "deepseek" => json!([
            { "description": "均衡", "effort": "medium" },
            { "description": "更强推理", "effort": "high" },
            { "description": "极限推理", "effort": "xhigh" }
        ]),
        "openrouter" => json!([
            { "description": "关闭推理", "effort": "none" },
            { "description": "极快", "effort": "minimal" },
            { "description": "更快", "effort": "low" },
            { "description": "均衡", "effort": "medium" },
            { "description": "更强推理", "effort": "high" },
            { "description": "极限推理", "effort": "xhigh" }
        ]),
        _ => json!([
            { "description": "更快", "effort": "low" },
            { "description": "均衡", "effort": "medium" },
            { "description": "更强推理", "effort": "high" },
            { "description": "极限推理", "effort": "xhigh" }
        ]),
    })
}

/// 厂商是否需要在 assistant tool_calls 历史上保留 `reasoning_content` 占位。
pub fn provider_needs_reasoning_content(provider: &ProviderConfig) -> bool {
    provider
        .codex_chat_reasoning_config()
        .and_then(|config| config.preserve_tool_call_reasoning)
        .unwrap_or(false)
}

fn infer_custom_reasoning_config(base_url: &str) -> Option<CodexChatReasoningConfig> {
    let base = base_url.to_ascii_lowercase();
    if base.contains("moonshot") || base.contains("kimi") {
        return Some(kimi_thinking());
    }
    if base.contains("bigmodel") || base.contains("zhipu") {
        return Some(zhipu_thinking());
    }
    if base.contains("deepseek") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(true),
            thinking_param: Some("thinking".into()),
            effort_param: Some("reasoning_effort".into()),
            effort_value_mode: Some("deepseek".into()),
            output_format: Some("reasoning_content".into()),
            preserve_tool_call_reasoning: Some(true),
            ..Default::default()
        });
    }
    if base.contains("openrouter") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(false),
            supports_effort: Some(true),
            thinking_param: Some("none".into()),
            effort_param: Some("reasoning.effort".into()),
            effort_value_mode: Some("openrouter".into()),
            output_format: Some("auto".into()),
            preserve_tool_call_reasoning: Some(false),
            ..Default::default()
        });
    }
    if base.contains("dashscope") || base.contains("aliyun") {
        return Some(thinking_only("enable_thinking", false));
    }
    if base.contains("minimax") {
        return Some(minimax_thinking());
    }
    if base.contains("mimo") {
        return Some(thinking_only("enable_thinking", true));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(id: &str, base_url: &str) -> ProviderConfig {
        ProviderConfig::new(id, id, base_url, "KEY", "model", "model", "responses")
    }

    #[test]
    fn deepseek_enables_thinking_and_effort() {
        let config = provider("deepseek", "https://api.deepseek.com/v1").codex_chat_reasoning_config();
        let config = config.unwrap();
        assert_eq!(config.supports_thinking, Some(true));
        assert_eq!(config.supports_effort, Some(true));
        assert_eq!(config.effort_value_mode.as_deref(), Some("deepseek"));
    }

    #[test]
    fn openrouter_custom_uses_native_reasoning_object() {
        let config = provider("custom", "https://openrouter.ai/api/v1")
            .codex_chat_reasoning_config()
            .unwrap();
        assert_eq!(config.effort_param.as_deref(), Some("reasoning.effort"));
        assert_eq!(config.thinking_param.as_deref(), Some("none"));
    }

    #[test]
    fn builtin_providers_use_vendor_native_thinking_shapes() {
        let minimax = provider("minimax", "https://api.minimaxi.com/v1")
            .codex_chat_reasoning_config()
            .unwrap();
        assert_eq!(minimax.thinking_type_when_enabled.as_deref(), Some("adaptive"));

        let zhipu = provider("zhipu", "https://open.bigmodel.cn/api/paas/v4")
            .codex_chat_reasoning_config()
            .unwrap();
        assert_eq!(zhipu.thinking_param.as_deref(), Some("thinking"));
        assert_eq!(zhipu.effort_param.as_deref(), Some("none"));

        let kimi = provider("kimi", "https://api.moonshot.cn/v1")
            .codex_chat_reasoning_config()
            .unwrap();
        assert_eq!(kimi.thinking_keep_when_enabled.as_deref(), Some("all"));
    }

    #[test]
    fn effort_levels_only_for_deepseek_and_openrouter() {
        assert!(provider_supports_reasoning_effort_levels(&provider(
            "deepseek",
            "https://api.deepseek.com/v1"
        )));
        assert!(!provider_supports_reasoning_effort_levels(&provider(
            "kimi",
            "https://api.moonshot.cn/v1"
        )));
        assert!(provider_supports_reasoning_effort_levels(&provider(
            "custom",
            "https://openrouter.ai/api/v1"
        )));
        assert!(supported_reasoning_levels_for_catalog(&provider(
            "qwen",
            "https://dashscope.aliyuncs.com/compatible-mode/v1"
        ))
        .is_none());
    }

    #[test]
    fn preserve_tool_call_reasoning_differs_for_qwen_and_mimo() {
        assert!(!provider_needs_reasoning_content(&provider(
            "qwen",
            "https://dashscope.aliyuncs.com/compatible-mode/v1"
        )));
        assert!(provider_needs_reasoning_content(&provider(
            "mimo",
            "https://api.xiaomimimo.com/v1"
        )));
    }
}
