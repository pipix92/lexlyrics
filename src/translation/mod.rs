use std::time::Duration;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

#[derive(Serialize)]
struct TranslateRequest<'a> {
    texts: &'a [String],
    tgt_lang: &'a str,
}

#[derive(Deserialize)]
struct TranslateResponse {
    translations: Vec<String>,
    latency_ms: Option<f64>,
}

/// Checks if a string contains Chinese characters
pub fn contains_chinese(text: &str) -> bool {
    text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

/// Sends pre-warm request to promote NLLB-200 to CUDA VRAM in ~0.7s
pub async fn wake_vram(base_url: &str) {
    let wake_url = if let Some(stripped) = base_url.strip_suffix("/translate") {
        format!("{stripped}/wake")
    } else {
        "http://127.0.0.1:18436/wake".to_string()
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(1500))
        .build();
    if let Ok(client) = client {
        let _ = client.get(&wake_url).send().await;
        info!("Sent wake signal to NLLB-200 daemon");
    }
}

/// Translates a batch of lyric lines using the configured backend
pub async fn translate_batch(
    lines: &[String],
    backend: &str,
    nllb_url: &str,
    target_lang: &str,
    timeout_ms: u64,
) -> Vec<String> {
    if lines.is_empty() {
        return Vec::new();
    }

    match backend {
        "nllb" => {
            if let Ok(res) = translate_nllb(lines, nllb_url, target_lang, timeout_ms).await {
                return res;
            }
            warn!("NLLB-200 backend failed, falling back to local CLI...");
            translate_cli_fallback(lines, "agy").await
        }
        "claude" => translate_cli_fallback(lines, "claude").await,
        "hermes" => translate_cli_fallback(lines, "hermes").await,
        "agy" => translate_cli_fallback(lines, "agy").await,
        _ => {
            if let Ok(res) = translate_nllb(lines, nllb_url, target_lang, timeout_ms).await {
                res
            } else {
                lines.to_vec()
            }
        }
    }
}

async fn translate_nllb(
    lines: &[String],
    url: &str,
    target_lang: &str,
    timeout_ms: u64,
) -> Result<Vec<String>, anyhow::Error> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()?;

    let req_body = TranslateRequest {
        texts: lines,
        tgt_lang: target_lang,
    };

    let resp = client.post(url).json(&req_body).send().await?;
    if resp.status().is_success() {
        let data: TranslateResponse = resp.json().await?;
        if let Some(lat) = data.latency_ms {
            info!("NLLB-200 translated {} lines in {:.1}ms", data.translations.len(), lat);
        }
        Ok(data.translations)
    } else {
        anyhow::bail!("NLLB server error: {}", resp.status())
    }
}

async fn translate_cli_fallback(lines: &[String], cli_name: &str) -> Vec<String> {
    let numbered_prompt = lines
        .iter()
        .enumerate()
        .map(|(i, l)| format!("{}: {}", i + 1, l))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "Traduci in italiano le seguenti righe di testo di canzone.\n\
         Mantieni rigorosamente il formato 'NUMERO: TESTO TRADOTTO'.\n\
         Non aggiungere commenti, note o testo extra:\n\
         {}",
        numbered_prompt
    );

    let output = match cli_name {
        "claude" => tokio::process::Command::new("claude")
            .arg("-p")
            .arg(&prompt)
            .output()
            .await,
        "hermes" => tokio::process::Command::new("hermes")
            .arg("-p")
            .arg(&prompt)
            .output()
            .await,
        _ => tokio::process::Command::new("agy")
            .arg("-p")
            .arg(&prompt)
            .output()
            .await,
    };

    if let Ok(out) = output {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let mut results = lines.to_vec();
            for line in stdout.lines() {
                if let Some((idx_str, trans)) = line.split_once(':') {
                    if let Ok(idx) = idx_str.trim().parse::<usize>() {
                        if idx >= 1 && idx <= results.len() {
                            results[idx - 1] = trans.trim().to_string();
                        }
                    }
                }
            }
            return results;
        }
    }

    lines.to_vec()
}
