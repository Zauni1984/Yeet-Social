//! F6 — sanctions screening for payout addresses (docs/mica/04 §B, 06 §8).
//!
//! Every address YEET is paid out to — the wallet a user links, the target of
//! a points→YEET conversion and each recipient of a batch mint — is checked
//! against a list of sanctioned EVM addresses before anything moves on-chain.
//!
//! List source: `SANCTIONS_LIST_URL` (default: the OFAC SDN digital-currency
//! addresses for Ethereum-compatible chains from the
//! 0xB10C/ofac-sanctioned-digital-currency-addresses mirror of the official
//! SDN list — BNB Smart Chain shares the EVM address space), optionally
//! merged with `SANCTIONS_LIST_FILE` (local newline-separated list) and
//! `SANCTIONS_EXTRA_ADDRESSES` (comma-separated, e.g. an internal blocklist).
//! Refreshed every `SANCTIONS_REFRESH_HOURS` (default 24).
//! `SANCTIONS_SCREENING=off` disables the check entirely (logged loudly).
//!
//! Fail-closed: until a list has loaded at least once, batch payouts are
//! *held* (not failed) and retried on the next run; after a successful load a
//! failing refresh keeps the last-good list.
use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tracing::{error, info, warn};

pub const DEFAULT_LIST_URL: &str =
    "https://raw.githubusercontent.com/0xB10C/ofac-sanctioned-digital-currency-addresses/lists/sanctioned_addresses_ETH.txt";

static LIST: RwLock<Option<Arc<HashSet<String>>>> = RwLock::new(None);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Not on the list (or screening disabled).
    Clear,
    /// On the list — never pay out to this address.
    Sanctioned,
    /// Screening is on but no list has loaded yet — hold, do not fail.
    Unknown,
}

pub fn enabled() -> bool {
    let v = std::env::var("SANCTIONS_SCREENING").unwrap_or_default();
    !matches!(v.trim().to_ascii_lowercase().as_str(), "off" | "0" | "false" | "no")
}

/// Lower-cased `0x` + 40 hex chars, or `None` for anything else.
pub fn normalize(addr: &str) -> Option<String> {
    let a = addr.trim().to_ascii_lowercase();
    if a.len() == 42 && a.starts_with("0x") && a[2..].chars().all(|c| c.is_ascii_hexdigit()) {
        Some(a)
    } else {
        None
    }
}

/// One address per line; `#` comments, blank lines and trailing CSV columns
/// are ignored, so the plain OFAC text export and simple CSVs both work.
pub fn parse_list(text: &str) -> HashSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| normalize(l.split(|c: char| c.is_whitespace() || c == ',' || c == ';').next().unwrap_or("")))
        .collect()
}

pub fn install(set: HashSet<String>) {
    if let Ok(mut g) = LIST.write() { *g = Some(Arc::new(set)); }
}

pub fn loaded_count() -> Option<usize> {
    LIST.read().ok().and_then(|g| g.as_ref().map(|s| s.len()))
}

pub fn check(addr: &str) -> Verdict {
    if !enabled() { return Verdict::Clear; }
    // Malformed addresses are rejected by the callers' own validation.
    let Some(a) = normalize(addr) else { return Verdict::Clear; };
    match LIST.read().ok().and_then(|g| g.clone()) {
        None => Verdict::Unknown,
        Some(set) if set.contains(&a) => Verdict::Sanctioned,
        Some(_) => Verdict::Clear,
    }
}

async fn load_once() -> anyhow::Result<usize> {
    let mut set: HashSet<String> = HashSet::new();
    let url = std::env::var("SANCTIONS_LIST_URL").ok()
        .map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_LIST_URL.to_string());
    if url != "none" {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("YEET-Social/1.0 sanctions")
            .build()?;
        let text = client.get(&url).send().await?.error_for_status()?.text().await?;
        let parsed = parse_list(&text);
        if parsed.is_empty() { anyhow::bail!("list at {url} parsed to zero addresses"); }
        set.extend(parsed);
    }
    if let Ok(path) = std::env::var("SANCTIONS_LIST_FILE") {
        if !path.trim().is_empty() {
            let text = tokio::fs::read_to_string(path.trim()).await?;
            set.extend(parse_list(&text));
        }
    }
    if let Ok(extra) = std::env::var("SANCTIONS_EXTRA_ADDRESSES") {
        set.extend(parse_list(&extra.replace(',', "\n")));
    }
    let n = set.len();
    install(set);
    Ok(n)
}

/// Startup + periodic refresh. Retries every 5 minutes while nothing has
/// loaded yet (payouts are held meanwhile), then every SANCTIONS_REFRESH_HOURS.
pub async fn start_sanctions_refresh() {
    if !enabled() {
        warn!("sanctions: screening DISABLED via SANCTIONS_SCREENING — payout addresses are not checked against any sanctions list");
        return;
    }
    let hours: u64 = std::env::var("SANCTIONS_REFRESH_HOURS").ok()
        .and_then(|v| v.parse().ok()).filter(|h: &u64| *h >= 1).unwrap_or(24);
    loop {
        match load_once().await {
            Ok(n) => info!("sanctions: list loaded ({n} addresses)"),
            Err(e) => {
                if loaded_count().is_none() {
                    error!("sanctions: no list loaded yet — batch payouts are HELD until one loads: {e}");
                } else {
                    warn!("sanctions: refresh failed, keeping the previous list: {e}");
                }
            }
        }
        let wait = if loaded_count().is_none() { Duration::from_secs(300) } else { Duration::from_secs(hours * 3600) };
        tokio::time::sleep(wait).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_rejects() {
        assert_eq!(normalize(" 0xABCDEF0123456789abcdef0123456789ABCDEF01 ").as_deref(), Some("0xabcdef0123456789abcdef0123456789abcdef01"));
        assert_eq!(normalize("abcdef0123456789abcdef0123456789abcdef01"), None); // no 0x
        assert_eq!(normalize("0x1234"), None);
        assert_eq!(normalize("0xZZcdef0123456789abcdef0123456789abcdef01"), None);
    }

    #[test]
    fn parses_text_and_csv_exports() {
        let text = "# OFAC SDN — digital currency addresses\n\n0x7F367cC41522cE07553e823bf3be79A889DEbe1B\n0x098B716B8Aaf21512996dC57EB0615e2383E2f96,ETH,some note\n0xdeadbeef ; junk\nnot an address\n";
        let set = parse_list(text);
        assert_eq!(set.len(), 2);
        assert!(set.contains("0x7f367cc41522ce07553e823bf3be79a889debe1b"));
        assert!(set.contains("0x098b716b8aaf21512996dc57eb0615e2383e2f96"));
    }

    #[test]
    fn verdicts_follow_the_installed_list() {
        std::env::remove_var("SANCTIONS_SCREENING");
        let mut set = HashSet::new();
        set.insert("0x7f367cc41522ce07553e823bf3be79a889debe1b".to_string());
        install(set);
        assert_eq!(check("0x7F367cC41522cE07553e823bf3be79A889DEbe1B"), Verdict::Sanctioned);
        assert_eq!(check("0x0000000000000000000000000000000000000001"), Verdict::Clear);
        assert_eq!(check("garbage"), Verdict::Clear);
        assert_eq!(loaded_count(), Some(1));
    }
}
