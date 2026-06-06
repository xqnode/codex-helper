//! 将 Codex Responses 的 `reasoning` 字段翻译为各厂商 Chat API 可识别的参数。

use serde_json::{json, Value};

use crate::config::ProviderConfig;
use crate::provider::codex_chat_reasoning::{codex_chat_reasoning_config_for, CodexChatReasoningConfig};

pub fn apply_reasoning_options(chat: &mut Value, provider: &ProviderConfig) {
    let Some(config) = codex_chat_reasoning_config_for(provider) else {
        return;
    };
    apply_reasoning_options_with_config(chat, &config);
}

fn apply_reasoning_options_with_config(chat: &mut Value, config: &CodexChatReasoningConfig) {
    let supports_effort = config.supports_effort.unwrap_or(false);
    let supports_thinking = config.supports_thinking.unwrap_or(false) || supports_effort;
    let Some(reasoning_enabled) = reasoning_requested(chat.get("reasoning")) else {
        return;
    };

    if supports_thinking {
        apply_thinking_param(chat, config, reasoning_enabled);
    }

    let effort_param = config
        .effort_param
        .as_deref()
        .unwrap_or("reasoning_effort")
        .trim()
        .to_ascii_lowercase();

    if !reasoning_enabled {
        if effort_param == "reasoning.effort" {
            chat["reasoning"] = json!({ "effort": "none" });
        } else {
            remove_reasoning_source(chat);
        }
        return;
    }

    if !supports_effort {
        remove_reasoning_source(chat);
        return;
    }

    let Some(effort) = chat
        .get("reasoning")
        .and_then(|value| value.get("effort"))
        .and_then(|v| v.as_str())
    else {
        remove_reasoning_source(chat);
        return;
    };

    let Some(mapped) = map_reasoning_effort(effort, config.effort_value_mode.as_deref()) else {
        remove_reasoning_source(chat);
        return;
    };

    match effort_param.as_str() {
        "reasoning_effort" => {
            chat["reasoning_effort"] = json!(mapped);
            remove_reasoning_source(chat);
        }
        "reasoning.effort" => {
            chat["reasoning"] = json!({ "effort": mapped });
        }
        _ => remove_reasoning_source(chat),
    }
}

fn apply_thinking_param(chat: &mut Value, config: &CodexChatReasoningConfig, reasoning_enabled: bool) {
    match config
        .thinking_param
        .as_deref()
        .unwrap_or("thinking")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "thinking" => {
            let enabled_type = config
                .thinking_type_when_enabled
                .as_deref()
                .unwrap_or("enabled");
            let mut thinking = serde_json::Map::new();
            thinking.insert(
                "type".into(),
                json!(if reasoning_enabled {
                    enabled_type
                } else {
                    "disabled"
                }),
            );
            if reasoning_enabled {
                if let Some(keep) = config.thinking_keep_when_enabled.as_ref() {
                    thinking.insert("keep".into(), json!(keep));
                }
                if let Some(clear) = config.thinking_clear_thinking_when_enabled {
                    thinking.insert("clear_thinking".into(), json!(clear));
                }
            }
            chat["thinking"] = Value::Object(thinking);
            if reasoning_enabled && config.reasoning_split_when_enabled == Some(true) {
                chat["reasoning_split"] = json!(true);
            }
        }
        "enable_thinking" => {
            chat["enable_thinking"] = json!(reasoning_enabled);
        }
        "reasoning_split" => {
            chat["reasoning_split"] = json!(reasoning_enabled);
        }
        _ => {}
    }
}

fn remove_reasoning_source(chat: &mut Value) {
    if let Some(obj) = chat.as_object_mut() {
        obj.remove("reasoning");
    }
}

fn reasoning_requested(reasoning: Option<&Value>) -> Option<bool> {
    let reasoning = reasoning?;
    if let Some(effort) = reasoning.get("effort").and_then(|v| v.as_str()) {
        return Some(!matches!(
            effort.trim().to_ascii_lowercase().as_str(),
            "none" | "off" | "disabled"
        ));
    }
    Some(!reasoning.is_null())
}

fn map_reasoning_effort(effort: &str, mode: Option<&str>) -> Option<&'static str> {
    let effort = effort.trim().to_ascii_lowercase();
    if matches!(effort.as_str(), "none" | "off" | "disabled") {
        return None;
    }

    match mode.unwrap_or("passthrough") {
        "deepseek" => match effort.as_str() {
            "max" | "xhigh" => Some("max"),
            _ => Some("high"),
        },
        "low_high" => match effort.as_str() {
            "minimal" | "low" => Some("low"),
            _ => Some("high"),
        },
        "openrouter" => match effort.as_str() {
            "max" | "xhigh" => Some("xhigh"),
            "high" => Some("high"),
            "medium" => Some("medium"),
            "low" => Some("low"),
            "minimal" => Some("minimal"),
            _ => None,
        },
        _ => match effort.as_str() {
            "minimal" => Some("minimal"),
            "low" => Some("low"),
            "medium" => Some("medium"),
            "high" => Some("high"),
            "xhigh" => Some("xhigh"),
            "max" => Some("max"),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deepseek() -> ProviderConfig {
        ProviderConfig::new(
            "deepseek",
            "DeepSeek",
            "https://api.deepseek.com/v1",
            "DEEPSEEK_API_KEY",
            "deepseek-v4-pro",
            "deepseek-v4-pro",
            "responses",
        )
    }

    fn minimax() -> ProviderConfig {
        ProviderConfig::new(
            "minimax",
            "Minimax",
            "https://api.minimaxi.com/v1",
            "MINIMAX_API_KEY",
            "minimax-m3",
            "MiniMax-M3",
            "responses",
        )
    }

    fn kimi() -> ProviderConfig {
        ProviderConfig::new(
            "kimi",
            "Kimi",
            "https://api.moonshot.cn/v1",
            "MOONSHOT_API_KEY",
            "kimi-k2.6",
            "kimi-k2.6",
            "responses",
        )
    }

    fn openrouter_custom() -> ProviderConfig {
        ProviderConfig::new(
            "custom",
            "OpenRouter",
            "https://openrouter.ai/api/v1",
            "OPENROUTER_API_KEY",
            "gpt-5.4",
            "gpt-5.4",
            "responses",
        )
    }

    #[test]
    fn maps_deepseek_reasoning_effort() {
        let mut chat = json!({
            "model": "deepseek-v4-pro",
            "reasoning": { "effort": "high" }
        });
        apply_reasoning_options(&mut chat, &deepseek());
        assert_eq!(chat["thinking"]["type"], "enabled");
        assert_eq!(chat["reasoning_effort"], "high");
        assert!(chat.get("reasoning").is_none());
    }

    #[test]
    fn minimax_uses_adaptive_thinking_type() {
        let mut chat = json!({
            "model": "MiniMax-M3",
            "reasoning": { "effort": "high" }
        });
        apply_reasoning_options(&mut chat, &minimax());
        assert_eq!(chat["thinking"]["type"], "adaptive");
        assert_eq!(chat["reasoning_split"], true);
        assert!(chat.get("reasoning").is_none());
        assert!(!chat.to_string().contains("enabled"));
    }

    #[test]
    fn minimax_explicit_none_uses_disabled_thinking_type() {
        let mut chat = json!({
            "model": "MiniMax-M3",
            "reasoning": { "effort": "none" }
        });
        apply_reasoning_options(&mut chat, &minimax());
        assert_eq!(chat["thinking"]["type"], "disabled");
        assert!(chat.get("reasoning_split").is_none());
    }

    #[test]
    fn maps_kimi_thinking_toggle() {
        let mut chat = json!({
            "model": "kimi-k2.6",
            "reasoning": { "effort": "medium" }
        });
        apply_reasoning_options(&mut chat, &kimi());
        assert_eq!(chat["thinking"]["type"], "enabled");
        assert!(chat.get("reasoning").is_none());
        assert!(chat.get("reasoning_effort").is_none());
    }

    #[test]
    fn maps_deepseek_max_effort_to_max() {
        let mut chat = json!({
            "model": "deepseek-v4-pro",
            "reasoning": { "effort": "max" }
        });
        apply_reasoning_options(&mut chat, &deepseek());
        assert_eq!(chat["reasoning_effort"], "max");
        assert_eq!(chat["thinking"]["type"], "enabled");
    }

    #[test]
    fn deepseek_explicit_none_disables_thinking_without_effort_alias() {
        let mut chat = json!({
            "model": "deepseek-v4-pro",
            "reasoning": { "effort": "none" }
        });
        apply_reasoning_options(&mut chat, &deepseek());
        assert_eq!(chat["thinking"]["type"], "disabled");
        assert!(chat.get("reasoning_effort").is_none());
        assert!(chat.get("reasoning").is_none());
    }

    #[test]
    fn openrouter_maps_max_to_xhigh_in_native_reasoning_object() {
        let mut chat = json!({
            "model": "deepseek/deepseek-chat-v3.1",
            "reasoning": { "effort": "max" }
        });
        apply_reasoning_options(&mut chat, &openrouter_custom());
        assert_eq!(chat["reasoning"]["effort"], "xhigh");
        assert!(chat.get("reasoning_effort").is_none());
        assert!(chat.get("thinking").is_none());
    }

    fn zhipu() -> ProviderConfig {
        ProviderConfig::new(
            "zhipu",
            "智谱",
            "https://open.bigmodel.cn/api/paas/v4",
            "ZHIPU_API_KEY",
            "glm-5.1",
            "glm-5.1",
            "responses",
        )
    }

    fn qwen() -> ProviderConfig {
        ProviderConfig::new(
            "qwen",
            "千问",
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
            "DASHSCOPE_API_KEY",
            "qwen3.7-max",
            "qwen3.7-max",
            "responses",
        )
    }

    fn mimo() -> ProviderConfig {
        ProviderConfig::new(
            "mimo",
            "小米 MiMo",
            "https://api.xiaomimimo.com/v1",
            "MIMO_API_KEY",
            "mimo-v2.5-pro",
            "mimo-v2.5-pro",
            "responses",
        )
    }

    #[test]
    fn zhipu_uses_thinking_type_not_reasoning_effort() {
        let mut chat = json!({
            "model": "glm-5.1",
            "reasoning": { "effort": "low" }
        });
        apply_reasoning_options(&mut chat, &zhipu());
        assert_eq!(chat["thinking"]["type"], "enabled");
        assert!(chat.get("reasoning_effort").is_none());
        assert!(chat.get("reasoning").is_none());
    }

    #[test]
    fn kimi_preserves_thinking_keep_for_tool_history() {
        let mut chat = json!({
            "model": "kimi-k2.6",
            "reasoning": { "effort": "high" }
        });
        apply_reasoning_options(&mut chat, &kimi());
        assert_eq!(chat["thinking"]["type"], "enabled");
        assert_eq!(chat["thinking"]["keep"], "all");
    }

    #[test]
    fn qwen_uses_enable_thinking_without_reasoning_effort() {
        let mut chat = json!({
            "model": "qwen3.7-max",
            "reasoning": { "effort": "medium" }
        });
        apply_reasoning_options(&mut chat, &qwen());
        assert_eq!(chat["enable_thinking"], true);
        assert!(chat.get("thinking").is_none());
        assert!(chat.get("reasoning_effort").is_none());
    }

    #[test]
    fn mimo_uses_enable_thinking_without_unsupported_fields() {
        let mut chat = json!({
            "model": "mimo-v2.5-pro",
            "reasoning": { "effort": "high" }
        });
        apply_reasoning_options(&mut chat, &mimo());
        assert_eq!(chat["enable_thinking"], true);
        assert!(chat.get("thinking").is_none());
        assert!(chat.get("reasoning_effort").is_none());
        assert!(chat.get("reasoning_split").is_none());
    }

    #[test]
    fn openrouter_passes_explicit_none_through() {
        let mut chat = json!({
            "model": "openai/gpt-5",
            "reasoning": { "effort": "none" }
        });
        apply_reasoning_options(&mut chat, &openrouter_custom());
        assert_eq!(chat["reasoning"]["effort"], "none");
        assert!(chat.get("reasoning_effort").is_none());
        assert!(chat.get("thinking").is_none());
    }
}
