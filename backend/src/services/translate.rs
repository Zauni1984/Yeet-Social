//! Post translation + language detection.
//!
//! Provider-agnostic: `TRANSLATE_PROVIDER=azure|google|deepl|libretranslate`
//! selects the backend, `TRANSLATE_URL` / `TRANSLATE_API_KEY` (and
//! `TRANSLATE_REGION` for Azure) configure it. Without a
//! provider everything is inert: the status endpoint reports `enabled:false`,
//! the UI hides the Translate button, and detection falls back to a free
//! stop-word heuristic so posts still carry a `lang` for the six UI
//! languages (used to decide whether a Translate button makes sense).
//!
//! Language codes are stored as lower-case ISO 639-1 (`en`, `de`, …);
//! `und` marks "checked, undetermined" so the sweep does not retry forever.
use std::time::Duration;
use serde::Deserialize;
use tokio::time::interval;
use tracing::{info, warn};
use uuid::Uuid;
use crate::AppState;

/// Languages the UI ships; also the whitelist for translation targets.
pub const SUPPORTED: [&str; 6] = ["en", "de", "it", "fr", "es", "pt"];
/// Sentinel for "detection ran, no confident result".
pub const UNDETERMINED: &str = "und";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider { LibreTranslate, DeepL, Azure, Google }

#[derive(Debug, Clone)]
pub struct TranslateConfig {
    pub provider: Option<Provider>,
    pub url: String,
    pub api_key: Option<String>,
    /// Azure resource region (e.g. `westeurope`, `germanywestcentral`).
    pub region: Option<String>,
}

impl TranslateConfig {
    pub fn enabled(&self) -> bool { self.provider.is_some() }
    pub fn provider_name(&self) -> Option<&'static str> {
        match self.provider {
            Some(Provider::LibreTranslate) => Some("libretranslate"), Some(Provider::DeepL) => Some("deepl"),
            Some(Provider::Azure) => Some("azure"), Some(Provider::Google) => Some("google"), None => None,
        }
    }
}

pub fn config() -> TranslateConfig {
    let raw = std::env::var("TRANSLATE_PROVIDER").ok().map(|s| s.trim().to_ascii_lowercase());
    let mut provider = match raw.as_deref() {
        Some("libretranslate") | Some("libre") => Some(Provider::LibreTranslate),
        Some("deepl") => Some(Provider::DeepL),
        Some("azure") | Some("microsoft") => Some(Provider::Azure),
        Some("google") | Some("gcp") => Some(Provider::Google),
        _ => None,
    };
    let api_key = std::env::var("TRANSLATE_API_KEY").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    if matches!(provider, Some(Provider::DeepL) | Some(Provider::Azure) | Some(Provider::Google)) && api_key.is_none() {
        warn!("translate: TRANSLATE_PROVIDER={} needs TRANSLATE_API_KEY — translation disabled", raw.as_deref().unwrap_or(""));
        provider = None;
    }
    let region = std::env::var("TRANSLATE_REGION").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let default_url = match provider {
        Some(Provider::DeepL) => "https://api.deepl.com",
        Some(Provider::LibreTranslate) => "http://libretranslate:5000",
        Some(Provider::Azure) => "https://api.cognitive.microsofttranslator.com",
        Some(Provider::Google) => "https://translation.googleapis.com",
        None => "",
    };
    let url = std::env::var("TRANSLATE_URL").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_url.to_string()).trim_end_matches('/').to_string();
    TranslateConfig { provider, url, api_key, region }
}

/// `de`, `DE`, `de-AT`, `pt_BR` → `de` / `pt`. Anything else → None.
pub fn normalize_lang(s: &str) -> Option<String> {
    let l = s.trim().to_ascii_lowercase();
    let l = l.split(['-', '_']).next().unwrap_or("");
    if l.len() == 2 && l.chars().all(|c| c.is_ascii_lowercase()) { Some(l.to_string()) } else { None }
}
pub fn is_supported(l: &str) -> bool { SUPPORTED.contains(&l) }
/// A stored `lang` that actually tells us something (not NULL, not `und`).
pub fn known_lang(l: Option<&str>) -> Option<String> {
    l.and_then(normalize_lang).filter(|x| x != UNDETERMINED)
}

pub struct Translation { pub text: String, pub source_lang: Option<String> }

fn client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder().timeout(Duration::from_secs(15)).user_agent("YEET-Social/1.0 translate").build()
}

pub async fn translate(cfg: &TranslateConfig, text: &str, target: &str, source: Option<&str>) -> anyhow::Result<Translation> {
    match cfg.provider {
        None => anyhow::bail!("translation disabled"),
        Some(Provider::LibreTranslate) => libre_translate(cfg, text, target, source).await,
        Some(Provider::DeepL) => deepl_translate(cfg, text, target, source).await,
        Some(Provider::Azure) => azure_translate(cfg, text, target, source).await,
        Some(Provider::Google) => google_translate(cfg, text, target, source).await,
    }
}

// ---- Azure AI Translator (F0: 2M chars/month free) ----
// https://learn.microsoft.com/azure/ai-services/translator/reference/v3-0-translate
fn azure_req(cfg: &TranslateConfig, path: &str) -> anyhow::Result<reqwest::RequestBuilder> {
    let mut r = client()?.post(format!("{}/{path}", cfg.url))
        .header("Ocp-Apim-Subscription-Key", cfg.api_key.clone().unwrap_or_default())
        .header("Content-Type", "application/json");
    if let Some(reg) = &cfg.region { r = r.header("Ocp-Apim-Subscription-Region", reg); }
    Ok(r)
}
async fn azure_translate(cfg: &TranslateConfig, text: &str, target: &str, source: Option<&str>) -> anyhow::Result<Translation> {
    #[derive(Deserialize)] struct Detected { language: String }
    #[derive(Deserialize)] struct Tr { text: String }
    #[derive(Deserialize)] struct Item { #[serde(rename = "detectedLanguage")] detected_language: Option<Detected>, translations: Vec<Tr> }
    let mut path = format!("translate?api-version=3.0&to={target}&textType=plain");
    if let Some(s) = source { path.push_str(&format!("&from={s}")); }
    let r = azure_req(cfg, &path)?.json(&serde_json::json!([{"Text": text}])).send().await?;
    if !r.status().is_success() {
        let st = r.status(); let t = r.text().await.unwrap_or_default();
        anyhow::bail!("azure http {st}: {}", t.chars().take(200).collect::<String>());
    }
    let items: Vec<Item> = r.json().await?;
    let item = items.into_iter().next().ok_or_else(|| anyhow::anyhow!("azure: empty response"))?;
    let tr = item.translations.into_iter().next().ok_or_else(|| anyhow::anyhow!("azure: no translation"))?;
    Ok(Translation { text: tr.text, source_lang: item.detected_language.and_then(|d| normalize_lang(&d.language)).or_else(|| source.map(|s| s.to_string())) })
}
async fn azure_detect(cfg: &TranslateConfig, text: &str) -> anyhow::Result<Option<String>> {
    #[derive(Deserialize)] struct D { language: String, score: f64 }
    let r = azure_req(cfg, "detect?api-version=3.0")?.json(&serde_json::json!([{"Text": text}])).send().await?;
    if !r.status().is_success() { anyhow::bail!("azure detect http {}", r.status()); }
    let list: Vec<D> = r.json().await?;
    Ok(list.into_iter().next().filter(|d| d.score >= 0.5).and_then(|d| normalize_lang(&d.language)))
}

// ---- Google Cloud Translation v2 (500k chars/month free) ----
// https://cloud.google.com/translate/docs/reference/rest/v2/translate
async fn google_translate(cfg: &TranslateConfig, text: &str, target: &str, source: Option<&str>) -> anyhow::Result<Translation> {
    #[derive(Deserialize)] struct Tr { #[serde(rename = "translatedText")] translated_text: String, #[serde(rename = "detectedSourceLanguage")] detected_source_language: Option<String> }
    #[derive(Deserialize)] struct Data { translations: Vec<Tr> }
    #[derive(Deserialize)] struct Resp { data: Data }
    let mut body = serde_json::json!({"q": text, "target": target, "format": "text"});
    if let Some(s) = source { body["source"] = serde_json::Value::String(s.to_string()); }
    let r = client()?.post(format!("{}/language/translate/v2", cfg.url))
        .query(&[("key", cfg.api_key.clone().unwrap_or_default())]).json(&body).send().await?;
    if !r.status().is_success() {
        let st = r.status(); let t = r.text().await.unwrap_or_default();
        anyhow::bail!("google http {st}: {}", t.chars().take(200).collect::<String>());
    }
    let resp: Resp = r.json().await?;
    let tr = resp.data.translations.into_iter().next().ok_or_else(|| anyhow::anyhow!("google: empty response"))?;
    Ok(Translation { text: tr.translated_text, source_lang: tr.detected_source_language.as_deref().and_then(normalize_lang).or_else(|| source.map(|s| s.to_string())) })
}
async fn google_detect(cfg: &TranslateConfig, text: &str) -> anyhow::Result<Option<String>> {
    #[derive(Deserialize)] struct D { language: String, #[serde(default)] confidence: f64 }
    #[derive(Deserialize)] struct Data { detections: Vec<Vec<D>> }
    #[derive(Deserialize)] struct Resp { data: Data }
    let r = client()?.post(format!("{}/language/translate/v2/detect", cfg.url))
        .query(&[("key", cfg.api_key.clone().unwrap_or_default())]).json(&serde_json::json!({"q": text})).send().await?;
    if !r.status().is_success() { anyhow::bail!("google detect http {}", r.status()); }
    let resp: Resp = r.json().await?;
    Ok(resp.data.detections.into_iter().flatten().next().filter(|d| d.confidence >= 0.5 || d.confidence == 0.0).and_then(|d| normalize_lang(&d.language)))
}

// ---- LibreTranslate (self-hosted or hosted; https://github.com/LibreTranslate/LibreTranslate) ----
async fn libre_translate(cfg: &TranslateConfig, text: &str, target: &str, source: Option<&str>) -> anyhow::Result<Translation> {
    #[derive(Deserialize)] struct Detected { language: String }
    #[derive(Deserialize)] struct Resp {
        #[serde(rename = "translatedText")] translated_text: String,
        #[serde(rename = "detectedLanguage")] detected_language: Option<Detected>,
    }
    let mut body = serde_json::json!({"q": text, "source": source.unwrap_or("auto"), "target": target, "format": "text"});
    if let Some(k) = &cfg.api_key { body["api_key"] = serde_json::Value::String(k.clone()); }
    let r = client()?.post(format!("{}/translate", cfg.url)).json(&body).send().await?;
    if !r.status().is_success() {
        let st = r.status(); let t = r.text().await.unwrap_or_default();
        anyhow::bail!("libretranslate http {st}: {}", t.chars().take(200).collect::<String>());
    }
    let resp: Resp = r.json().await?;
    let detected = resp.detected_language.and_then(|d| normalize_lang(&d.language));
    Ok(Translation { text: resp.translated_text, source_lang: detected.or_else(|| source.map(|s| s.to_string())) })
}

async fn libre_detect(cfg: &TranslateConfig, text: &str) -> anyhow::Result<Option<String>> {
    #[derive(Deserialize)] struct D { language: String, confidence: f64 }
    let mut body = serde_json::json!({"q": text});
    if let Some(k) = &cfg.api_key { body["api_key"] = serde_json::Value::String(k.clone()); }
    let r = client()?.post(format!("{}/detect", cfg.url)).json(&body).send().await?;
    if !r.status().is_success() { anyhow::bail!("libretranslate detect http {}", r.status()); }
    let list: Vec<D> = r.json().await?;
    // LibreTranslate reports confidence 0..100; below ~40 it is guessing.
    Ok(list.into_iter().find(|d| d.confidence >= 40.0).and_then(|d| normalize_lang(&d.language)))
}

// ---- DeepL (https://developers.deepl.com/docs/api-reference/translate) ----
fn deepl_target(target: &str) -> String {
    match target { "en" => "EN-US".into(), "pt" => "PT-BR".into(), other => other.to_ascii_uppercase() }
}
async fn deepl_translate(cfg: &TranslateConfig, text: &str, target: &str, source: Option<&str>) -> anyhow::Result<Translation> {
    #[derive(Deserialize)] struct Item { detected_source_language: Option<String>, text: String }
    #[derive(Deserialize)] struct Resp { translations: Vec<Item> }
    let mut body = serde_json::json!({"text": [text], "target_lang": deepl_target(target)});
    if let Some(s) = source { body["source_lang"] = serde_json::Value::String(s.to_ascii_uppercase()); }
    let r = client()?.post(format!("{}/v2/translate", cfg.url))
        .header("Authorization", format!("DeepL-Auth-Key {}", cfg.api_key.clone().unwrap_or_default()))
        .json(&body).send().await?;
    if !r.status().is_success() {
        let st = r.status(); let t = r.text().await.unwrap_or_default();
        anyhow::bail!("deepl http {st}: {}", t.chars().take(200).collect::<String>());
    }
    let resp: Resp = r.json().await?;
    let item = resp.translations.into_iter().next().ok_or_else(|| anyhow::anyhow!("deepl: empty response"))?;
    Ok(Translation { text: item.text, source_lang: item.detected_source_language.as_deref().and_then(normalize_lang).or_else(|| source.map(|s| s.to_string())) })
}

/// Best available detection: provider (LibreTranslate has /detect) first,
/// then the free heuristic. `None` = no confident answer.
pub async fn detect(cfg: &TranslateConfig, text: &str) -> Option<String> {
    // The free heuristic first: it costs nothing and is right for most
    // posts in the six UI languages. Only ask the provider (which counts
    // against the free quota) when the heuristic has no confident answer.
    if let Some(l) = heuristic_detect(text) { return Some(l); }
    let via = match cfg.provider {
        Some(Provider::LibreTranslate) => libre_detect(cfg, text).await,
        Some(Provider::Azure) => azure_detect(cfg, text).await,
        Some(Provider::Google) => google_detect(cfg, text).await,
        _ => Ok(None),
    };
    match via {
        Ok(l) => l,
        Err(e) => { warn!("translate: detect via provider failed: {e}"); None }
    }
}

/// Cheap stop-word detector for the six UI languages. Deliberately
/// conservative: needs a clear winner with at least two hits, otherwise
/// `None`. Wrong guesses only cost a needless Translate button, missing
/// guesses only cost auto-translation — so err on the side of `None`.
pub fn heuristic_detect(text: &str) -> Option<String> {
    const EN: &[&str] = &["the","and","is","are","you","that","this","with","for","have","not","but","was","from","they","what","your","just","about","it's","i'm","don't","will","can","my"];
    const DE: &[&str] = &["und","der","die","das","ist","nicht","ich","ein","eine","mit","auf","für","sich","wir","ihr","auch","dem","den","noch","aber","wie","schon","mal","heute","kein","keine","habe","bin","sind","wird"];
    const IT: &[&str] = &["che","non","per","una","con","sono","questo","della","del","gli","anche","come","più","nel","alla","ho","hai","ma","cosa","oggi","è","il","lo","la","di","tutti"];
    const FR: &[&str] = &["les","des","est","pas","une","que","pour","dans","avec","vous","nous","sur","je","c'est","mais","très","tout","aussi","ce","au","du","le","la","et","un"];
    const ES: &[&str] = &["que","los","las","por","una","con","para","del","está","pero","como","más","este","esta","hoy","muy","también","yo","es","el","y","un","no","en","lo"];
    const PT: &[&str] = &["não","uma","com","para","você","que","isso","mais","muito","está","também","hoje","ele","ela","nós","um","os","as","do","da","é","o","e","em","de"];
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !(c.is_alphabetic() || c == '\''))
        .filter(|w| !w.is_empty())
        .collect();
    if words.len() < 3 { return None; }
    let score = |list: &[&str]| words.iter().filter(|w| list.contains(w)).count();
    let mut scores = [("en", score(EN)), ("de", score(DE)), ("it", score(IT)), ("fr", score(FR)), ("es", score(ES)), ("pt", score(PT))];
    scores.sort_by_key(|a| std::cmp::Reverse(a.1));
    let (best, top) = scores[0];
    let second = scores[1].1;
    if top >= 2 && top > second { Some(best.to_string()) } else { None }
}

/// Detect + store the language of one freshly created post (fire-and-forget).
pub fn spawn_detect(state: AppState, post_id: Uuid, content: String) {
    tokio::spawn(async move {
        let cfg = config();
        let lang = detect(&cfg, &content).await.unwrap_or_else(|| UNDETERMINED.to_string());
        if let Err(e) = sqlx::query("UPDATE posts SET lang = $2 WHERE id = $1 AND lang IS NULL")
            .bind(post_id).bind(&lang).execute(state.db.pool()).await
        {
            warn!("translate: store lang for {post_id}: {e}");
        }
    });
}

/// Background sweep: every 30 s tag posts that still have `lang IS NULL`
/// (scheduled publishes, reposts, RSS webboards, legacy rows). Batches of
/// 100, newest first; heuristic is free, provider detection is one HTTP
/// call per post.
pub async fn start_lang_sweep(state: AppState) {
    let mut tick = interval(Duration::from_secs(30));
    info!("translate: language sweep active (provider: {})", config().provider_name().unwrap_or("none/heuristic"));
    loop {
        tick.tick().await;
        let cfg = config();
        let rows: Vec<(Uuid, String)> = match sqlx::query_as(
            "SELECT id, content FROM posts WHERE lang IS NULL AND deleted_at IS NULL ORDER BY created_at DESC LIMIT 100"
        ).fetch_all(state.db.pool()).await {
            Ok(r) => r,
            Err(e) => { warn!("translate: sweep query: {e}"); continue; }
        };
        for (id, content) in rows {
            let lang = detect(&cfg, &content).await.unwrap_or_else(|| UNDETERMINED.to_string());
            if let Err(e) = sqlx::query("UPDATE posts SET lang = $2 WHERE id = $1 AND lang IS NULL")
                .bind(id).bind(&lang).execute(state.db.pool()).await
            {
                warn!("translate: sweep store {id}: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_codes() {
        assert_eq!(normalize_lang("DE").as_deref(), Some("de"));
        assert_eq!(normalize_lang("pt-BR").as_deref(), Some("pt"));
        assert_eq!(normalize_lang("en_US").as_deref(), Some("en"));
        assert_eq!(normalize_lang("und"), None);
        assert_eq!(normalize_lang(""), None);
        assert_eq!(normalize_lang("deu"), None);
        assert_eq!(known_lang(Some("und")), None);
        assert_eq!(known_lang(Some("De")).as_deref(), Some("de"));
        assert_eq!(known_lang(None), None);
    }

    #[test]
    fn provider_parsing() {
        std::env::set_var("TRANSLATE_PROVIDER", "azure"); std::env::set_var("TRANSLATE_API_KEY", "k"); std::env::set_var("TRANSLATE_REGION", "westeurope");
        let c = config();
        assert_eq!(c.provider, Some(Provider::Azure)); assert_eq!(c.url, "https://api.cognitive.microsofttranslator.com"); assert_eq!(c.region.as_deref(), Some("westeurope"));
        std::env::set_var("TRANSLATE_PROVIDER", "google"); std::env::remove_var("TRANSLATE_API_KEY");
        assert_eq!(config().provider, None, "google without key must disable");
        std::env::remove_var("TRANSLATE_PROVIDER"); std::env::remove_var("TRANSLATE_REGION");
        assert!(!config().enabled());
    }

    #[test]
    fn deepl_targets() {
        assert_eq!(deepl_target("en"), "EN-US");
        assert_eq!(deepl_target("pt"), "PT-BR");
        assert_eq!(deepl_target("de"), "DE");
    }

    #[test]
    fn heuristic_detects_ui_languages() {
        assert_eq!(heuristic_detect("Ich habe heute keine Lust auf die Arbeit, aber es muss sein.").as_deref(), Some("de"));
        assert_eq!(heuristic_detect("This is the best day and I'm not even joking about it.").as_deref(), Some("en"));
        assert_eq!(heuristic_detect("Oggi non ho voglia di lavorare, ma è la cosa giusta da fare.").as_deref(), Some("it"));
        assert_eq!(heuristic_detect("Je ne sais pas pour vous, mais c'est très bien aussi.").as_deref(), Some("fr"));
        assert_eq!(heuristic_detect("Hoy no tengo ganas de trabajar, pero es lo que hay.").as_deref(), Some("es"));
        assert_eq!(heuristic_detect("Hoje não estou com vontade de trabalhar, mas você sabe como é.").as_deref(), Some("pt"));
    }

    #[test]
    fn heuristic_is_conservative() {
        assert_eq!(heuristic_detect("gm"), None);
        assert_eq!(heuristic_detect("#btc #eth 🚀🚀🚀"), None);
        assert_eq!(heuristic_detect("0x1234 abcdef zzz"), None);
    }
}
