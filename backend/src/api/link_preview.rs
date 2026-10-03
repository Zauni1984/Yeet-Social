//! Server-side Open Graph preview for links in posts.
//!
//! Hardened after the 2026-10 audit (C-H4): the endpoint used to fetch any
//! URL anonymously, follow redirects and buffer the whole body — an SSRF and
//! internal-network oracle. Now: signed-in callers only, rate limited, the
//! host is resolved first and must be a public address (the resolved IP is
//! pinned for the request so DNS cannot change underneath), no redirects,
//! HTML only, 512 KB cap.
use axum::{extract::{Query, State}, http::HeaderMap, Json};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use crate::{AppError, AppResult, AppState};
use crate::api::middleware::AuthUser;

#[derive(Deserialize)]
pub struct PreviewQuery {
    pub url: String,
}

#[derive(Serialize, Default)]
pub struct LinkPreview {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub image: Option<String>,
    pub site_name: Option<String>,
}

const MAX_BODY: usize = 512 * 1024;

pub async fn get_link_preview(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Query(params): Query<PreviewQuery>,
) -> AppResult<Json<LinkPreview>> {
    let url = params.url.trim().to_string();
    let empty = LinkPreview { url: url.clone(), ..Default::default() };
    if url.len() > 2048 || !(url.starts_with("http://") || url.starts_with("https://")) {
        return Ok(Json(empty));
    }
    // Per user and per IP: previews are cheap for us to refuse and expensive
    // (outbound fetch) to serve.
    crate::api::middleware::limit(&state, "linkpreview_user", &auth.address, 60, 20, 3600, 200).await?;
    let ip = crate::api::middleware::client_ip(&headers);
    crate::api::middleware::limit(&state, "linkpreview_ip", &ip, 60, 40, 3600, 400).await?;

    match fetch_og_tags(&url).await {
        Ok(preview) => Ok(Json(preview)),
        Err(_) => Ok(Json(empty)),
    }
}

/// Host part of an http(s) URL without userinfo, port or path.
fn host_and_port(url: &str) -> Option<(String, u16)> {
    let (default_port, rest) = match (url.strip_prefix("https://"), url.strip_prefix("http://")) {
        (Some(r), _) => (443u16, r),
        (None, Some(r)) => (80u16, r),
        (None, None) => return None,
    };
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?; // drop userinfo
    if authority.is_empty() { return None; }
    if let Some(stripped) = authority.strip_prefix('[') {
        // IPv6 literal — never public for our purposes (we only preview DNS names).
        let _ = stripped; return None;
    }
    let mut it = authority.splitn(2, ':');
    let host = it.next()?.to_ascii_lowercase();
    let port = match it.next() { Some(p) => p.parse().ok()?, None => default_port };
    if host.is_empty() || host.parse::<IpAddr>().is_ok() { return None; } // IP literals refused
    if host == "localhost" || host.ends_with(".local") || host.ends_with(".internal") || host.ends_with(".localhost") || !host.contains('.') {
        return None;
    }
    Some((host, port))
}

/// True for globally routable unicast addresses only.
pub(crate) fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            !(v.is_private() || v.is_loopback() || v.is_link_local() || v.is_broadcast()
                || v.is_documentation() || v.is_unspecified() || v.is_multicast()
                || o[0] == 0
                || (o[0] == 100 && (64..128).contains(&o[1]))   // CGNAT 100.64/10
                || (o[0] == 192 && o[1] == 0 && o[2] == 0)       // IETF protocol assignments
                || o[0] >= 240)                                  // reserved / class E
        }
        IpAddr::V6(v) => {
            if let Some(m) = v.to_ipv4_mapped() { return is_public_ip(IpAddr::V4(m)); }
            let s = v.segments();
            !(v.is_loopback() || v.is_unspecified() || v.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00   // ULA fc00::/7
                || (s[0] & 0xffc0) == 0xfe80   // link-local fe80::/10
                || s[0] == 0x2001 && s[1] == 0x0db8) // documentation
        }
    }
}

async fn fetch_og_tags(url: &str) -> Result<LinkPreview, Box<dyn std::error::Error + Send + Sync>> {
    let (host, port) = host_and_port(url).ok_or("host not allowed")?;
    // Resolve once, require a public address, and pin it for the request.
    let mut addrs = tokio::net::lookup_host((host.as_str(), port)).await?;
    let addr: SocketAddr = addrs.next().ok_or("unresolvable")?;
    if !is_public_ip(addr.ip()) {
        return Err("private address".into());
    }
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (compatible; YEETBot/1.0)")
        .timeout(std::time::Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .resolve(&host, addr)
        .build()?;

    let mut response = client.get(url).send().await?;
    if !response.status().is_success() {
        return Err("non-2xx".into());
    }
    let ct = response.headers().get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    if !ct.contains("text/html") && !ct.contains("application/xhtml") {
        return Err("not html".into());
    }
    let mut body: Vec<u8> = Vec::with_capacity(16 * 1024);
    while let Some(chunk) = response.chunk().await? {
        if body.len() + chunk.len() > MAX_BODY {
            body.extend_from_slice(&chunk[..MAX_BODY - body.len()]);
            break;
        }
        body.extend_from_slice(&chunk);
    }
    let html = String::from_utf8_lossy(&body);

    let document = scraper::Html::parse_document(&html);
    let meta_sel = scraper::Selector::parse("meta").unwrap();
    let title_sel = scraper::Selector::parse("title").unwrap();

    let mut og: HashMap<String, String> = HashMap::new();
    for meta in document.select(&meta_sel) {
        let name = meta.value().attr("property")
            .or_else(|| meta.value().attr("name"))
            .unwrap_or("")
            .to_lowercase();
        if let Some(content) = meta.value().attr("content") {
            og.insert(name, content.chars().take(1000).collect());
        }
    }

    let title = og.get("og:title")
        .or_else(|| og.get("twitter:title"))
        .cloned()
        .or_else(|| {
            document.select(&title_sel)
                .next()
                .map(|t| t.text().collect::<String>().trim().chars().take(300).collect())
        });
    let description = og.get("og:description")
        .or_else(|| og.get("twitter:description"))
        .or_else(|| og.get("description"))
        .cloned();
    // Only https images from public hosts are passed back to the client.
    let image = og.get("og:image")
        .or_else(|| og.get("twitter:image"))
        .cloned()
        .filter(|u| u.starts_with("https://") && host_and_port(u).is_some());
    let site_name = og.get("og:site_name").cloned().or_else(|| Some(host.trim_start_matches("www.").to_string()));

    Ok(LinkPreview { url: url.to_string(), title, description, image, site_name })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_internal_hosts_and_literals() {
        assert!(host_and_port("http://127.0.0.1/").is_none());
        assert!(host_and_port("http://169.254.169.254/latest/meta-data").is_none());
        assert!(host_and_port("http://localhost:5000/").is_none());
        assert!(host_and_port("http://libretranslate/").is_none());
        assert!(host_and_port("http://[::1]/").is_none());
        assert!(host_and_port("https://user:pw@example.com/x").is_some());
        assert_eq!(host_and_port("https://Example.com:8443/a?b").unwrap(), ("example.com".into(), 8443));
    }
    #[test]
    fn public_ip_classification() {
        assert!(!is_public_ip("10.1.2.3".parse().unwrap()));
        assert!(!is_public_ip("172.16.5.5".parse().unwrap()));
        assert!(!is_public_ip("192.168.0.1".parse().unwrap()));
        assert!(!is_public_ip("100.64.0.1".parse().unwrap()));
        assert!(!is_public_ip("127.0.0.1".parse().unwrap()));
        assert!(!is_public_ip("169.254.169.254".parse().unwrap()));
        assert!(!is_public_ip("::1".parse().unwrap()));
        assert!(!is_public_ip("fd00::1".parse().unwrap()));
        assert!(!is_public_ip("::ffff:10.0.0.1".parse().unwrap()));
        assert!(is_public_ip("93.184.216.34".parse().unwrap()));
        assert!(is_public_ip("2606:4700::1111".parse().unwrap()));
    }
}
