use anyhow::Result;
use chrono::Utc;
use serde::Deserialize;
use tokio::time::{interval, Duration};
use tracing::{info, error};
use uuid::Uuid;

use crate::AppState;

/// Syncs all active web board connections every 15 minutes.
/// Fetches RSS/Atom feeds and inserts new posts into the posts table
/// with source_type='web_board' so they appear in users' feeds.
pub async fn start_webboard_sync(state: AppState) {
    let mut ticker = interval(Duration::from_secs(900)); // 15 min
    loop {
        ticker.tick().await;
        if let Err(e) = sync_all_boards(&state).await {
            error!("Web board sync error: {e}");
        }
    }
}

async fn sync_all_boards(state: &AppState) -> Result<()> {
    #[allow(dead_code)]
    struct BoardRow {
        id: uuid::Uuid,
        user_id: uuid::Uuid,
        domain: String,
        feed_url: String,
        username: Option<String>,
    }
    let boards: Vec<BoardRow> = sqlx::query_as!(
        BoardRow,
        r#"SELECT id as "id: uuid::Uuid", user_id as "user_id: uuid::Uuid", domain, feed_url, username
        FROM webboard_connections WHERE is_active = true"#
    )
    .fetch_all(&state.db.pool)
    .await?;

    info!("Syncing {} web board connections...", boards.len());

    for board in boards {
        if let Err(e) = sync_board(state, &board.feed_url, &board.domain, board.user_id).await {
            error!("Failed to sync {}: {e}", board.domain);
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct RssItem {
    title: Option<String>,
    description: Option<String>,
    link: Option<String>,
    pub_date: Option<String>,
}

async fn sync_board(
    state: &AppState,
    feed_url: &str,
    domain: &str,
    owner_id: Uuid,
) -> Result<()> {
    let client = reqwest::Client::builder()
        .user_agent("Yeet-SocialBot/1.0")
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let body = client.get(feed_url).send().await?.text().await?;

    // Parse RSS/Atom (simplified â production would use a proper crate)
    let items = parse_rss_simple(&body);

    for item in items.iter().take(10) { // max 10 new posts per sync
        let content = format!(
            "ð¡ **{}** | {}\n{}",
            domain,
            item.title.as_deref().unwrap_or(""),
            item.description.as_deref().unwrap_or(""),
        );

        // Upsert by link to avoid duplicates
        sqlx::query_unchecked!(
            r#"
            INSERT INTO posts (
                id, author_id, content, visibility,
                source_type, source_domain,
                expires_at, created_at
            )
            VALUES ($1, $2, $3, 'public', 'web_board', $4, NOW() + INTERVAL '24 hours', NOW())
            ON CONFLICT DO NOTHING
            "#,
            Uuid::new_v4(),
            owner_id,
            content,
            domain,
        )
        .execute(&state.db.pool)
        .await?;
    }

    Ok(())
}

/// Very minimal RSS parser â extracts <item> blocks
fn parse_rss_simple(xml: &str) -> Vec<RssItem> {
    let mut items = Vec::new();
    for chunk in xml.split("<item>").skip(1) {
        let end = chunk.find("</item>").unwrap_or(chunk.len());
        let item_xml = &chunk[..end];

        items.push(RssItem {
            title:       extract_tag(item_xml, "title"),
            description: extract_tag(item_xml, "description"),
            link:        extract_tag(item_xml, "link"),
            pub_date:    extract_tag(item_xml, "pubDate"),
        });
    }
    items
}

fn extract_tag(xml: &str, tag: &str) -> Option<String> {
    let open  = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end   = xml[start..].find(&close)?;
    let raw = xml[start..start + end].trim();
    // Strip CDATA if present
    let clean = raw
        .trim_start_matches("<![CDATA[")
        .trim_end_matches("]]>")
        .trim();
    Some(clean.to_string())
}
