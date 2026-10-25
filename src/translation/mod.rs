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

/// NetEase credit lines look like `鼓编程 : Michael Keenan` (a short Chinese
/// role, a colon, a name) and keep slipping into non-Chinese songs.
pub fn is_credit_line(text: &str) -> bool {
    let Some((role, name)) = text.split_once([':', '：']) else {
        return false;
    };
    let (role, name) = (role.trim(), name.trim());
    // a Chinese "name" longer than this is a verse that happens to have a colon
    let name_fits = !contains_chinese(name) || name.chars().count() <= 12;
    !role.is_empty()
        && !name.is_empty()
        && name_fits
        && role.chars().count() <= 12
        && role
            .chars()
            .all(|c| ('\u{4e00}'..='\u{9fff}').contains(&c) || c.is_whitespace() || c == '/')
}

/// Blank the credit lines out, keeping them as gaps so the timing is intact.
pub fn blank_credit_lines(lyric: crate::lyric_providers::LyricOwned) -> crate::lyric_providers::LyricOwned {
    use crate::lyric_providers::LyricOwned;

    match lyric {
        LyricOwned::LineTimestamp(mut lines) => {
            for line in lines.iter_mut().filter(|line| is_credit_line(&line.text)) {
                line.text.clear();
            }
            LyricOwned::LineTimestamp(lines)
        }
        other => other,
    }
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

/// Whether `target_lang` (NLLB or ISO code) means Italian
pub fn is_italian(target_lang: &str) -> bool {
    matches!(target_lang, "ita_Latn" | "it" | "ita")
}

/// ISO 639-1 code for the external API from an NLLB code like `deu_Latn`;
/// ISO codes pass through untouched.
pub fn iso_code(target_lang: &str) -> String {
    const NLLB_TO_ISO: [(&str, &str); 16] = [
        ("ita_Latn", "it"),
        ("eng_Latn", "en"),
        ("deu_Latn", "de"),
        ("fra_Latn", "fr"),
        ("spa_Latn", "es"),
        ("por_Latn", "pt"),
        ("nld_Latn", "nl"),
        ("pol_Latn", "pl"),
        ("ron_Latn", "ro"),
        ("rus_Cyrl", "ru"),
        ("ukr_Cyrl", "uk"),
        ("tur_Latn", "tr"),
        ("jpn_Jpan", "ja"),
        ("kor_Hang", "ko"),
        ("zho_Hans", "zh-CN"),
        ("arb_Arab", "ar"),
    ];
    NLLB_TO_ISO
        .iter()
        .find(|(nllb, _)| *nllb == target_lang)
        .map(|(_, iso)| iso.to_string())
        .unwrap_or_else(|| target_lang.to_string())
}

/// Translates a batch of lyric lines using the configured backend.
///
/// Italian goes through the local pipeline (NLLB-200, then the CLI agents).
/// Any other target language is a work in progress and is sent to the
/// external API for now; so is everything when `backend = "api"`.
pub async fn translate_batch(lines: &[String], config: &crate::config::TranslationConfig) -> Vec<String> {
    if lines.is_empty() {
        return Vec::new();
    }

    let crate::config::TranslationConfig {
        backend,
        nllb_url,
        target_lang,
        timeout_ms,
        ..
    } = config;

    if backend == "api" || !is_italian(target_lang) {
        return match translate_external(lines, config).await {
            Ok(res) => res,
            Err(e) => {
                warn!("external translation API failed: {e}");
                lines.to_vec()
            }
        };
    }

    match backend.as_str() {
        "nllb" => {
            if let Ok(res) = translate_nllb(lines, nllb_url, target_lang, *timeout_ms).await {
                return res;
            }
            warn!("NLLB-200 backend failed, falling back to local CLI...");
            translate_cli_fallback(lines, "agy").await
        }
        "claude" => translate_cli_fallback(lines, "claude").await,
        "hermes" => translate_cli_fallback(lines, "hermes").await,
        "agy" => translate_cli_fallback(lines, "agy").await,
        _ => {
            if let Ok(res) = translate_nllb(lines, nllb_url, target_lang, *timeout_ms).await {
                res
            } else {
                lines.to_vec()
            }
        }
    }
}

#[derive(Deserialize)]
struct MyMemoryResponse {
    #[serde(rename = "responseStatus")]
    status: serde_json::Value,
    #[serde(rename = "responseData")]
    data: MyMemoryData,
}

#[derive(Deserialize)]
struct MyMemoryData {
    #[serde(rename = "translatedText")]
    text: String,
}

/// MyMemory takes at most 500 bytes per request
const EXTERNAL_MAX_BYTES: usize = 450;

/// Work in progress: translate through an external API (MyMemory by default).
///
/// Lines are joined with newlines, which the API keeps, so a whole chunk of a
/// song costs one request; a chunk whose line count does not survive the round
/// trip is retried line by line.
async fn translate_external(
    lines: &[String],
    config: &crate::config::TranslationConfig,
) -> Result<Vec<String>, anyhow::Error> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(config.timeout_ms.max(5000)))
        .build()?;
    let target = iso_code(&config.target_lang);

    let request = |text: String| {
        let mut query = vec![
            ("q", text),
            ("langpair", format!("autodetect|{target}")),
        ];
        if !config.api_email.is_empty() {
            query.push(("de", config.api_email.clone()));
        }
        let client = client.clone();
        let url = config.api_url.clone();
        async move {
            let url = reqwest::Url::parse_with_params(&url, &query)?;
            let response: MyMemoryResponse = client.get(url).send().await?.json().await?;
            if response.status != 200 && response.status != "200" {
                anyhow::bail!("{}: {}", response.status, response.data.text);
            }
            Ok::<_, anyhow::Error>(response.data.text)
        }
    };

    // group lines into requests that stay under the size limit
    let mut chunks: Vec<&[String]> = vec![];
    let (mut start, mut bytes) = (0, 0);
    for (idx, line) in lines.iter().enumerate() {
        if idx > start && bytes + line.len() + 1 > EXTERNAL_MAX_BYTES {
            chunks.push(&lines[start..idx]);
            (start, bytes) = (idx, 0);
        }
        bytes += line.len() + 1;
    }
    chunks.push(&lines[start..]);

    let mut translated = Vec::with_capacity(lines.len());
    for chunk in chunks {
        let joined = request(chunk.join("\n")).await?;
        let split: Vec<String> = joined.split('\n').map(|l| l.trim().to_string()).collect();
        if split.len() == chunk.len() {
            translated.extend(split);
            continue;
        }
        for line in chunk {
            translated.push(request(line.clone()).await?.trim().to_string());
        }
    }

    info!(
        "external API translated {} lines to {target} (work in progress)",
        translated.len()
    );
    Ok(translated)
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

const REFINE_PROMPT: &str = "Sei un traduttore di testi musicali. Traduci in italiano il testo \
della canzone \"{title}\" di {artist}.
Regole:
- italiano naturale e scorrevole, come lo direbbe un madrelingua, non parola per parola;
- modi di dire, metafore e giochi di parole: rendi il significato con un'espressione italiana equivalente;
- parolacce, slang e volgarità: mantienile con un equivalente italiano di pari forza, senza censurare né attenuare;
- nomi propri, marchi e parti già in italiano restano invariati;
- tieni conto del contesto di tutta la canzone.
Rispondi SOLO con le righe tradotte nel formato NUMERO: TESTO, una per riga, \
con gli stessi numeri, senza commenti.

{lines}";

/// Second pass: an LLM CLI rewrites the whole song in natural Italian, fixing
/// what NLLB-200 renders literally (slang, swearing, idioms).
///
/// Returns one entry per input line; `None` where the model skipped a line.
pub async fn refine_song(
    lines: &[String],
    title: &str,
    artist: &str,
    config: &crate::config::TranslationConfig,
) -> Result<Vec<Option<String>>, anyhow::Error> {
    let program = match config.refine_backend.as_str() {
        program @ ("agy" | "hermes" | "claude") => program,
        other => anyhow::bail!("unknown refine backend {other}"),
    };

    let numbered = lines
        .iter()
        .enumerate()
        .map(|(i, line)| format!("{}: {line}", i + 1))
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = REFINE_PROMPT
        .replace("{title}", title)
        .replace("{artist}", artist)
        .replace("{lines}", &numbered);

    let mut command = tokio::process::Command::new(program);
    command
        .arg("-p")
        .arg(prompt)
        .stdin(std::process::Stdio::null())
        // keep the agent away from any project it could pick up from the cwd
        .current_dir(std::env::temp_dir())
        .kill_on_drop(true);

    let output = tokio::time::timeout(
        Duration::from_secs(config.refine_timeout_s),
        command.output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("{program} took more than {}s", config.refine_timeout_s))??;
    if !output.status.success() {
        anyhow::bail!("{program} exited with {}", output.status);
    }

    let refined = parse_numbered(&String::from_utf8_lossy(&output.stdout), lines.len());

    // a reply that lost most lines is not a translation of this song
    let filled = refined.iter().filter(|line| line.is_some()).count();
    if filled * 5 < lines.len() * 4 {
        anyhow::bail!("{program} returned {filled} of {} lines", lines.len());
    }

    info!("{program} refined {filled} lines");
    Ok(refined)
}

/// Parse `NUMBER: TEXT` lines (tolerating markdown bullets and `NUMBER.`)
/// into `count` slots, `None` where a number is missing.
fn parse_numbered(reply: &str, count: usize) -> Vec<Option<String>> {
    let mut parsed = vec![None; count];
    for line in reply.lines() {
        let line = line.trim().trim_start_matches(['*', '-', ' ']);
        let Some((idx, text)) = line.split_once([':', '.']) else {
            continue;
        };
        let (Ok(idx), text) = (idx.trim().trim_end_matches('*').parse::<usize>(), text.trim()) else {
            continue;
        };
        if (1..=count).contains(&idx) && !text.is_empty() {
            parsed[idx - 1] = Some(text.trim_start_matches("**").trim().to_string());
        }
    }
    parsed
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

#[cfg(test)]
mod tests {
    use super::is_credit_line;

    #[test]
    fn credit_lines() {
        assert!(is_credit_line("鼓编程 : Michael Keenan"));
        assert!(is_credit_line("人声 : Alexandria"));
        assert!(is_credit_line("作词：周杰伦"));
        assert!(is_credit_line("制作人/编曲 : Someone"));
    }

    #[test]
    fn numbered_reply() {
        let reply = "Ecco:\n1: Cazzo, era la mia ora\n**2.** Venderei l'anima: marcia\n- 4: Spacca tutto\n9: fuori range";
        let parsed = super::parse_numbered(reply, 4);
        assert_eq!(parsed[0].as_deref(), Some("Cazzo, era la mia ora"));
        assert_eq!(parsed[1].as_deref(), Some("Venderei l'anima: marcia"));
        assert_eq!(parsed[2], None);
        assert_eq!(parsed[3].as_deref(), Some("Spacca tutto"));
    }

    #[test]
    fn lyric_lines_are_kept() {
        assert!(!is_credit_line("Verse 1: here we go"));
        assert!(!is_credit_line("Please step in line"));
        assert!(!is_credit_line("我爱你：永远不变的心意在这里陪着你走过"));
        assert!(!is_credit_line(""));
    }
}
