use axum::{extract::Query, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize)]
pub struct PreviewQuery {
    pub url: String,
}

#[derive(Serialize)]
pub struct LinkPreview {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub image: Option<String>,
    pub site_name: Option<String>,
}

pub async fn get_link_preview(Query(params): Query<PreviewQuery>) -> impl IntoResponse {
    let url = params.url.clone();

    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Json(LinkPreview { url, title: None, description: None, image: None, site_name: None });
    }

    match fetch_og_tags(&url).await {
        Ok(preview) => Json(preview),
        Err(_) => Json(LinkPreview { url, title: None, description: None, image: None, site_name: None }),
    }
}

fn extract_hostname(url: &str) -> Option<String> {
    // Strip protocol
    let without_proto = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    // Take up to first slash or end
    let host = without_proto.split('/').next()?;
    // Strip www.
    let host = host.strip_prefix("www.").unwrap_or(host);
    Some(host.to_string())
}

async fn fetch_og_tags(url: &str) -> Result<LinkPreview, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (compatible; YEETBot/1.0)")
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    let response = client.get(url).send().await?;
    let html = response.text().await?;

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
            og.insert(name, content.to_string());
        }
    }

    let title = og.get("og:title")
        .or_else(|| og.get("twitter:title"))
        .cloned()
        .or_else(|| {
            document.select(&title_sel)
                .next()
                .map(|t| t.text().collect::<String>().trim().to_string())
        });

    let description = og.get("og:description")
        .or_else(|| og.get("twitter:description"))
        .or_else(|| og.get("description"))
        .cloned();

    let image = og.get("og:image")
        .or_else(|| og.get("twitter:image"))
        .cloned();

    let site_name = og.get("og:site_name")
        .cloned()
        .or_else(|| extract_hostname(url));

    Ok(LinkPreview {
        url: url.to_string(),
        title,
        description,
        image,
        site_name,
    })
}
