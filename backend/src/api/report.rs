use axum::{extract::{Path, Query, State}, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::{api::middleware::AuthUser, error::{AppError, AppResult}, models::ApiResponse, state::AppState};

#[derive(Deserialize)]
pub struct ReportRequest {
    pub reason: Option<String>,
    pub details: Option<String>,
}

#[derive(Deserialize)]
pub struct AdminQuery {
    pub secret: Option<String>,
    pub page: Option<i64>,
}

#[derive(Serialize)]
pub struct ReportedPost {
    pub id: Uuid,
    pub content: String,
    pub author_id: Option<Uuid>,
    pub author_name: Option<String>,
    pub report_count: i32,
    pub is_flagged: bool,
    pub is_removed: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub removed_reason: Option<String>,
}

#[derive(Deserialize)]
pub struct RemoveRequest {
    pub secret: String,
    pub reason: Option<String>,
}

/// POST /api/v1/posts/:id/report — authenticated users can report a post
pub async fn report_post(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(post_id): Path<Uuid>,
    Json(req): Json<ReportRequest>,
) -> AppResult<Json<ApiResponse<()>>> {
    // Resolve reporter user id
    let reporter_id: Option<Uuid> = if let Some(uuid_str) = auth.address.strip_prefix("email:") {
        uuid_str.parse::<Uuid>().ok()
    } else {
        sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
            .bind(&auth.address)
            .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
    };

    // Check post exists and is not already removed
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM posts WHERE id = $1 AND is_removed = FALSE)")
        .bind(post_id)
        .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
    if !exists {
        return Err(AppError::NotFound("Post not found".into()));
    }

    // Prevent duplicate reports from same user
    if let Some(rid) = reporter_id {
        let already: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM post_reports WHERE post_id = $1 AND reporter_id = $2)"
        ).bind(post_id).bind(rid)
         .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
        if already {
            return Ok(Json(ApiResponse::ok(())));
        }
    }

    let reason = req.reason.unwrap_or_else(|| "inappropriate".to_string());
    sqlx::query(
        "INSERT INTO post_reports (post_id, reporter_id, reason, details) VALUES ($1, $2, $3, $4)"
    )
    .bind(post_id).bind(reporter_id)
    .bind(&reason).bind(&req.details)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(())))
}

fn check_admin(secret: &str) -> AppResult<()> {
    let admin_secret = std::env::var("ADMIN_SECRET").unwrap_or_else(|_| "yeet_admin_2024".to_string());
    if secret != admin_secret {
        return Err(AppError::Unauthorised("Invalid admin secret".into()));
    }
    Ok(())
}

/// GET /api/v1/admin/posts?secret=X&page=1 — list all posts with report info
pub async fn admin_list_posts(
    State(state): State<AppState>,
    Query(q): Query<AdminQuery>,
) -> AppResult<Json<ApiResponse<Vec<ReportedPost>>>> {
    check_admin(q.secret.as_deref().unwrap_or(""))?;
    let page = q.page.unwrap_or(1).max(1);
    let offset = (page - 1) * 50;
    let posts = sqlx::query_as!(ReportedPost,
        r#"SELECT p.id, p.content, p.author_id,
            u.display_name as author_name,
            p.report_count, p.is_flagged, p.is_removed,
            p.created_at, p.removed_reason
           FROM posts p
           LEFT JOIN users u ON u.id = p.author_id
           ORDER BY p.is_flagged DESC, p.report_count DESC, p.created_at DESC
           LIMIT 50 OFFSET $1"#, offset
    ).fetch_all(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(posts)))
}

/// GET /api/v1/admin/reports?secret=X — list flagged posts only
pub async fn admin_list_reports(
    State(state): State<AppState>,
    Query(q): Query<AdminQuery>,
) -> AppResult<Json<ApiResponse<Vec<ReportedPost>>>> {
    check_admin(q.secret.as_deref().unwrap_or(""))?;
    let posts = sqlx::query_as!(ReportedPost,
        r#"SELECT p.id, p.content, p.author_id,
            u.display_name as author_name,
            p.report_count, p.is_flagged, p.is_removed,
            p.created_at, p.removed_reason
           FROM posts p
           LEFT JOIN users u ON u.id = p.author_id
           WHERE p.is_flagged = TRUE AND p.is_removed = FALSE
           ORDER BY p.report_count DESC, p.created_at DESC"#
    ).fetch_all(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(posts)))
}

/// DELETE /api/v1/admin/posts/:id — permanently remove a post
pub async fn admin_remove_post(
    State(state): State<AppState>,
    Path(post_id): Path<Uuid>,
    Json(req): Json<RemoveRequest>,
) -> AppResult<Json<ApiResponse<()>>> {
    check_admin(&req.secret)?;
    sqlx::query(
        "UPDATE posts SET is_removed = TRUE, removed_at = NOW(), removed_reason = $1 WHERE id = $2"
    )
    .bind(&req.reason).bind(post_id)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(())))
}

/// DELETE /api/v1/admin/posts/:id/hard — hard delete (permanent)
pub async fn admin_hard_delete_post(
    State(state): State<AppState>,
    Path(post_id): Path<Uuid>,
    Json(req): Json<RemoveRequest>,
) -> AppResult<Json<ApiResponse<()>>> {
    check_admin(&req.secret)?;
    sqlx::query("DELETE FROM posts WHERE id = $1")
        .bind(post_id)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(())))
}

/// POST /api/v1/admin/posts/:id/unflag — clear flag
pub async fn admin_unflag_post(
    State(state): State<AppState>,
    Path(post_id): Path<Uuid>,
    Json(req): Json<RemoveRequest>,
) -> AppResult<Json<ApiResponse<()>>> {
    check_admin(&req.secret)?;
    sqlx::query(
        "UPDATE posts SET is_flagged = FALSE, report_count = 0 WHERE id = $1"
    )
    .bind(post_id)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(())))
}
