use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::sync::RwLock;
use uuid::Uuid;

const MAX_ENTRIES: usize = 300;

#[derive(Clone, Debug, Serialize)]
pub struct RequestLogEntry {
    pub id: String,
    pub time_ms: i64,
    pub time_label: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    pub path: String,
    pub stream: bool,
    pub status: u16,
    pub duration_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_yuan: Option<f64>,
    pub cost_label: String,
    pub ok: bool,
}

#[derive(Clone, Debug)]
pub struct PendingRequest {
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    pub path: String,
    pub stream: bool,
    pub started: Instant,
    pub status: u16,
}

#[derive(Clone, Debug, Default)]
pub struct UsageSnapshot {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Clone)]
pub struct RequestLogStore {
    inner: Arc<RwLock<VecDeque<RequestLogEntry>>>,
}

impl RequestLogStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(VecDeque::with_capacity(64))),
        }
    }

    pub async fn push(&self, entry: RequestLogEntry) {
        let mut entries = self.inner.write().await;
        entries.push_back(entry);
        while entries.len() > MAX_ENTRIES {
            entries.pop_front();
        }
    }

    pub async fn list(&self) -> Vec<RequestLogEntry> {
        let entries = self.inner.read().await;
        entries.iter().rev().cloned().collect()
    }

    pub async fn clear(&self) {
        self.inner.write().await.clear();
    }

    pub async fn summary(&self) -> RequestLogSummary {
        let entries = self.inner.read().await;
        let mut total_in = 0u64;
        let mut total_out = 0u64;
        let mut total_cost = 0.0f64;
        let mut cost_known = 0usize;
        for entry in entries.iter() {
            total_in += entry.input_tokens;
            total_out += entry.output_tokens;
            if let Some(cost) = entry.cost_yuan {
                total_cost += cost;
                cost_known += 1;
            }
        }
        RequestLogSummary {
            count: entries.len(),
            total_input_tokens: total_in,
            total_output_tokens: total_out,
            total_cost_yuan: if cost_known > 0 {
                Some(total_cost)
            } else {
                None
            },
        }
    }

    pub fn finalize(&self, pending: PendingRequest, usage: Option<UsageSnapshot>) -> RequestLogEntry {
        let usage = usage.unwrap_or_default();
        let duration_ms = pending.started.elapsed().as_millis() as u64;
        let cost_yuan = estimate_cost_yuan(
            &pending.provider_id,
            &pending.model,
            usage.input_tokens,
            usage.output_tokens,
        );
        let cost_label = cost_yuan
            .map(format_cost_yuan)
            .unwrap_or_else(|| "—".into());
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        RequestLogEntry {
            id: Uuid::new_v4().to_string(),
            time_ms: now,
            time_label: format_time_label(now),
            provider_id: pending.provider_id,
            provider_name: pending.provider_name,
            model: pending.model,
            path: pending.path,
            stream: pending.stream,
            status: pending.status,
            duration_ms,
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: usage.total_tokens,
            cost_yuan,
            cost_label,
            ok: pending.status >= 200 && pending.status < 400,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RequestLogSummary {
    pub count: usize,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cost_yuan: Option<f64>,
}

pub fn parse_usage_from_json(value: &serde_json::Value) -> Option<UsageSnapshot> {
    let usage = value.get("usage")?;
    parse_usage_object(usage)
}

pub fn parse_usage_from_bytes(bytes: &[u8]) -> Option<UsageSnapshot> {
    let mut latest = None;
    for line in bytes.split(|b| *b == b'\n') {
        let line = line.strip_prefix(b"data: ").or_else(|| {
            line.strip_prefix(b"data:")
                .map(|rest| rest.strip_prefix(b" ").unwrap_or(rest))
        });
        let Some(line) = line else { continue };
        if line == b"[DONE]" {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(usage) = parse_usage_from_json(&value) {
            latest = Some(usage);
        }
    }
    latest
}

fn parse_usage_object(usage: &serde_json::Value) -> Option<UsageSnapshot> {
    if !usage.is_object() {
        return None;
    }
    let input_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total_tokens = usage
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(input_tokens + output_tokens);
    if input_tokens == 0 && output_tokens == 0 && total_tokens == 0 {
        return None;
    }
    Some(UsageSnapshot {
        input_tokens,
        output_tokens,
        total_tokens,
    })
}

/// 公开价估算（元 / 百万 tokens），未知模型返回 None。
fn estimate_cost_yuan(
    provider_id: &str,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
) -> Option<f64> {
    if input_tokens == 0 && output_tokens == 0 {
        return Some(0.0);
    }
    let (in_per_m, out_per_m) = pricing_per_million(provider_id, model)?;
    Some(
        (input_tokens as f64 * in_per_m + output_tokens as f64 * out_per_m) / 1_000_000.0,
    )
}

fn pricing_per_million(provider_id: &str, model: &str) -> Option<(f64, f64)> {
    let model = model.to_ascii_lowercase();
    match provider_id {
        "deepseek" if model.contains("flash") => Some((0.5, 2.0)),
        "deepseek" if model.contains("pro") => Some((2.0, 8.0)),
        "qwen" if model.contains("plus") => Some((0.8, 2.0)),
        "qwen" if model.contains("max") => Some((2.0, 6.0)),
        "zhipu" if model.contains("glm-5") => Some((1.0, 3.0)),
        "zhipu" if model.contains("4.7") => Some((0.5, 1.5)),
        "kimi" => Some((1.0, 3.0)),
        "minimax" => Some((1.0, 3.0)),
        "mimo" if model.contains("flash") => Some((0.3, 1.0)),
        "mimo" => Some((1.0, 3.0)),
        "custom" if model.contains("mini") => Some((0.5, 1.5)),
        "custom" if model.contains("5.5") || model.contains("5.4") => Some((2.0, 8.0)),
        _ => None,
    }
}

fn format_cost_yuan(yuan: f64) -> String {
    if yuan < 0.01 {
        format!("约 ¥{:.4}", yuan)
    } else {
        format!("约 ¥{:.3}", yuan)
    }
}

fn format_time_label(time_ms: i64) -> String {
    let secs = time_ms / 1000;
    let ms = (time_ms % 1000) as u32;
    let local = secs % 86400;
    let h = local / 3600;
    let m = (local % 3600) / 60;
    let s = local % 60;
    format!("{:02}:{:02}:{:02}.{:03}", h, m, s, ms)
}

pub fn extract_model_from_body(body: &[u8], fallback: &str) -> String {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return fallback.to_string();
    };
    value
        .get("model")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_usage_from_sse_chunk() {
        let chunk = br#"data: {"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}

"#;
        let usage = parse_usage_from_bytes(chunk).unwrap();
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 5);
    }
}
