//! Webboard / RSS board handlers.
use axum::{extract::State, Json};
use serde::Serialize;
use crate::{AppResult, AppState, models::ApiResponse};

#[derive(Debug, Serialize)]
pub struct Board {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub rss_url: &'static str,
    pub icon: &'static str,
    pub category: &'static str,
}

const BOARDS: &[Board] = &[
    Board {
        id: "cointelegraph",
        name: "CoinTelegraph",
        description: "Crypto news & analysis",
        rss_url: "https://cointelegraph.com/rss",
        icon: "CT",
        category: "news",
    },
    Board {
        id: "decrypt",
        name: "Decrypt",
        description: "Web3 news & analysis",
        rss_url: "https://decrypt.co/feed",
        icon: "DC",
        category: "news",
    },
    Board {
        id: "thedefiant",
        name: "The Defiant",
        description: "DeFi news",
        rss_url: "https://thedefiant.io/feed",
        icon: "DF",
        category: "defi",
    },
    Board {
        id: "nftnow",
        name: "NFT Now",
        description: "NFT news & drops",
        rss_url: "https://nftnow.com/feed/",
        icon: "NN",
        category: "nft",
    },
];

pub async fn get_boards(
    State(_state): State<AppState>,
) -> AppResult<Json<ApiResponse<Vec<serde_json::Value>>>> {
    let boards: Vec<serde_json::Value> = BOARDS.iter().map(|b| serde_json::json!({
        "id": b.id,
        "name": b.name,
        "description": b.description,
        "rss_url": b.rss_url,
        "icon": b.icon,
        "category": b.category,
    })).collect();

    Ok(Json(ApiResponse::ok(boards)))
}

pub async fn get_board(
    State(_state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> AppResult<Json<ApiResponse<serde_json::Value>>> {
    let board = BOARDS.iter().find(|b| b.id == id.as_str())
        .ok_or_else(|| crate::AppError::NotFound("Board not found".into()))?;

    Ok(Json(ApiResponse::ok(serde_json::json!({
        "id": board.id,
        "name": board.name,
        "description": board.description,
        "rss_url": board.rss_url,
        "icon": board.icon,
        "category": board.category,
    }))))
}
