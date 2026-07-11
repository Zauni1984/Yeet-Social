use anyhow::Result;
use ethers::{
    prelude::*,
    providers::{Http, Provider},
    signers::LocalWallet,
};
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{info, error, warn};

use crate::AppState;

#[derive(sqlx::FromRow)]
struct DueScheduledPost {
    id: uuid::Uuid,
    author_id: uuid::Uuid,
    content: String,
    media_url: Option<String>,
    is_adult: bool,
    is_nft: bool,
    nft_price_yeet: Option<f64>,
    is_permanent: bool,
    ppv_price_yeet: Option<f64>,
    publish_at: chrono::DateTime<chrono::Utc>,
}

/// Batch reward job: runs every hour, collects pending off-chain
/// rewards from DB and submits a single batchMintRewards tx to BSC.
/// This keeps gas costs minimal (1 tx per hour instead of per action).
pub async fn start_reward_batch_job(state: AppState) {
    let privkey = match std::env::var("REWARDS_MINTER_PRIVKEY") {
        Ok(k) if !k.is_empty() => k,
        _ => {
            warn!("REWARDS_MINTER_PRIVKEY not set â batch reward job disabled");
            return;
        }
    };

    let mut ticker = interval(Duration::from_secs(3600)); // every hour

    loop {
        ticker.tick().await;
        info!("Running batch reward mint job...");

        if let Err(e) = run_batch(&state, &privkey).await {
            error!("Batch reward job failed: {e}");
        }
    }
}

async fn run_batch(state: &AppState, privkey: &str) -> Result<()> {
    // Fetch all unminted rewards (tx_hash IS NULL) from DB
    #[allow(dead_code)]
    struct RewardRow {
        id: uuid::Uuid,
        wallet_address: Option<String>,
        action: Option<String>,
        amount: Option<f64>,
    }
    let rows: Vec<RewardRow> = sqlx::query_as!(
        RewardRow,
        r#"SELECT r.id as "id: uuid::Uuid", u.wallet_address, r.action::text as action, r.amount::float8 as amount
        FROM token_rewards r JOIN users u ON u.id = r.user_id
        WHERE r.tx_hash IS NULL AND u.wallet_address IS NOT NULL
        ORDER BY r.created_at ASC LIMIT 500"#
    )
    .fetch_all(&state.db.pool)
    .await?;

    if rows.is_empty() {
        info!("No pending rewards to mint.");
        return Ok(());
    }

    info!("Minting {} reward records on BSC...", rows.len());

    // Build arrays for batchMintRewards(address[], uint256[], string[]) in a
    // SINGLE pass so recipients/amounts/actions can never desync, and track
    // exactly which reward ids made it into the transaction so we only mark
    // those minted. Previously `recipients` used filter_map while
    // amounts/actions iterated every row — a future SQL edit dropping the
    // `wallet_address IS NOT NULL` guard would have minted the wrong amount
    // to the wrong address, and all ids were marked minted regardless.
    let mut recipients: Vec<Address> = Vec::with_capacity(rows.len());
    let mut amounts: Vec<U256> = Vec::with_capacity(rows.len());
    let mut actions: Vec<String> = Vec::with_capacity(rows.len());
    let mut included_ids: Vec<uuid::Uuid> = Vec::with_capacity(rows.len());
    for r in &rows {
        let Some(wallet) = r.wallet_address.as_ref() else { continue; };
        let addr = match wallet.parse::<Address>() {
            Ok(a) => a,
            Err(e) => { warn!("batch-rewards: skip reward {} — bad wallet {wallet}: {e}", r.id); continue; }
        };
        let yeet: f64 = r.amount.unwrap_or(0.0);
        if !yeet.is_finite() || yeet < 0.0 {
            warn!("batch-rewards: skip reward {} — invalid amount {yeet}", r.id);
            continue;
        }
        let wei = (yeet * 1e18) as u128; // non-negative + finite, checked above
        recipients.push(addr);
        amounts.push(U256::from(wei));
        actions.push(r.action.clone().unwrap_or_default());
        included_ids.push(r.id);
    }

    if recipients.is_empty() {
        info!("No mintable rewards after validation.");
        return Ok(());
    }

    // Set up signer. Chain id comes from env so the backend and the
    // frontend (window.YEET_CHAIN) can be kept in lock-step; default 56
    // (BSC Mainnet) matches the default RPC below.
    let chain_id: u64 = std::env::var("YEET_CHAIN_ID")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(56);
    let wallet: LocalWallet = privkey.parse::<LocalWallet>()?
        .with_chain_id(chain_id);

    let provider = Provider::<Http>::try_from(
        std::env::var("BSC_RPC_URL")
            .unwrap_or_else(|_| "https://bsc-dataseed.binance.org/".into())
    )?;
    let client = Arc::new(SignerMiddleware::new(provider, wallet));

    // Call batchMintRewards on YeetToken contract
    let token_addr: Address = std::env::var("YEET_TOKEN_ADDRESS")?.parse()?;

    abigen!(
    YeetToken,
    r#"[{"inputs":[{"name":"recipients","type":"address[]"},{"name":"amounts","type":"uint256[]"},{"name":"actions","type":"string[]"}],"name":"batchMintRewards","outputs":[],"stateMutability":"nonpayable","type":"function"}]"#
);

    let contract = YeetToken::new(token_addr, client);
    let tx = contract
        .batch_mint_rewards(recipients, amounts, actions)
        .gas(500_000u64)
        .send()
        .await?
        .await?
        .ok_or_else(|| anyhow::anyhow!("No receipt"))?;

    let tx_hash = format!("{:?}", tx.transaction_hash);
    info!("Batch mint tx: {}", tx_hash);

    // Mark ONLY the rows actually included in the transaction.
    sqlx::query!(
        "UPDATE token_rewards SET tx_hash = $1 WHERE id = ANY($2)",
        tx_hash,
        &included_ids,
    )
    .execute(&state.db.pool)
    .await?;

    info!("Marked {} rewards as minted.", included_ids.len());
    Ok(())
}

/// Also runs a cleanup job to soft-delete expired non-NFT posts
pub async fn start_cleanup_job(state: AppState) {
    let mut ticker = interval(Duration::from_secs(300)); // every 5 minutes
    let uploads_root = std::env::var("UPLOADS_DIR").unwrap_or_else(|_| "/app/uploads".into());
    loop {
        ticker.tick().await;
        match sqlx::query_scalar!(
            r#"SELECT cleanup_expired_posts() as "count!""#
        )
        .fetch_one(&state.db.pool)
        .await
        {
            Ok(n) if n > 0 => info!("Cleaned up {} expired posts.", n),
            Ok(_) => {}
            Err(e) => error!("Cleanup job error: {e}"),
        }

        // Orphan-sweep posts-media/. Any file in that dir whose URL is
        // no longer referenced from posts.media_url is removed — that
        // covers both posts deleted by `cleanup_expired_posts` above
        // and uploads where the user never hit "YEET IT".
        // Guard: only touch files older than ~30 min to avoid racing
        // an in-progress upload (multipart save → INSERT into posts).
        sweep_post_media_orphans(&state, &uploads_root).await;
    }
}

async fn sweep_post_media_orphans(state: &AppState, uploads_root: &str) {
    let dir = std::path::Path::new(uploads_root).join("posts-media");
    let mut rd = match tokio::fs::read_dir(&dir).await {
        Ok(rd) => rd,
        // posts-media/ is created lazily on first upload — missing is fine.
        Err(_) => return,
    };

    let referenced: std::collections::HashSet<String> = match sqlx::query_scalar::<_, String>(
        "SELECT media_url FROM posts
          WHERE media_url LIKE '/uploads/posts-media/%'"
    ).fetch_all(&state.db.pool).await {
        Ok(v) => v.into_iter().collect(),
        Err(e) => { warn!("posts-media sweep: DB scan error: {e}"); return; }
    };

    let cutoff = std::time::SystemTime::now() - Duration::from_secs(30 * 60);
    let mut removed: u64 = 0;
    while let Ok(Some(entry)) = rd.next_entry().await {
        let path = entry.path();
        let fname = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let url = format!("/uploads/posts-media/{fname}");
        if referenced.contains(&url) { continue; }
        // Skip very-recent files — they may belong to a post-in-flight.
        if let Ok(meta) = entry.metadata().await {
            if let Ok(modified) = meta.modified() {
                if modified > cutoff { continue; }
            }
        }
        if let Err(e) = tokio::fs::remove_file(&path).await {
            warn!("Failed to remove orphan post-media {}: {e}", path.display());
        } else {
            removed += 1;
        }
    }
    if removed > 0 {
        info!("posts-media sweep: removed {removed} orphan files.");
    }
}

/// Hard-cap retention for encrypted messages: anything past
/// `expires_at` (server-set to created_at + 30 days) is purged from
/// the database. Runs hourly. Per-user retention shorter than 30 d
/// is enforced client-side as a display filter on top of this.
///
/// For image messages we collect the on-disk blob paths first and
/// unlink them after the DB rows are gone — otherwise we'd leak
/// orphaned ciphertext blobs into UPLOADS_DIR/dm-blobs/.
pub async fn start_message_cleanup_job(state: AppState) {
    let mut ticker = interval(Duration::from_secs(3600));
    let uploads_root = std::env::var("UPLOADS_DIR").unwrap_or_else(|_| "/app/uploads".into());
    loop {
        ticker.tick().await;

        // 1. Snapshot expired image blob paths before deleting rows.
        let expired_paths: Vec<String> = match sqlx::query_scalar(
            "SELECT blob_path FROM messages
              WHERE expires_at < NOW() AND blob_path IS NOT NULL"
        ).fetch_all(&state.db.pool).await {
            Ok(v) => v,
            Err(e) => { error!("Expired-blob scan error: {e}"); Vec::new() }
        };

        match sqlx::query("DELETE FROM messages WHERE expires_at < NOW()")
            .execute(&state.db.pool).await
        {
            Ok(r) if r.rows_affected() > 0 => {
                info!("Message cleanup: purged {} expired messages.", r.rows_affected())
            }
            Ok(_) => {}
            Err(e) => error!("Message cleanup error: {e}"),
        }

        // 2. Unlink blobs whose rows just vanished.
        for rel in expired_paths {
            if rel.contains("..") { continue; } // defensive: never escape
            let full = std::path::Path::new(&uploads_root).join(&rel);
            if let Err(e) = tokio::fs::remove_file(&full).await {
                warn!("Failed to remove orphaned blob {}: {e}", full.display());
            }
        }

        // 3. Old pending invitations.
        if let Err(e) = sqlx::query(
            "DELETE FROM group_invitations
              WHERE status = 'pending'
                AND created_at < NOW() - INTERVAL '30 days'"
        ).execute(&state.db.pool).await {
            warn!("Old-invitations purge error: {e}");
        }

        // 4. Scrub disclosed_plaintext from resolved message reports
        //    older than 90 days. Metadata stays for abuse-pattern
        //    analysis; the content is purged so a future DB leak
        //    can't expose old DMs that someone reported.
        if let Err(e) = sqlx::query(
            "UPDATE message_reports
                SET disclosed_plaintext = NULL
              WHERE status <> 'pending'
                AND resolved_at IS NOT NULL
                AND resolved_at < NOW() - INTERVAL '90 days'
                AND disclosed_plaintext IS NOT NULL"
        ).execute(&state.db.pool).await {
            warn!("Report plaintext scrub error: {e}");
        }

        // 5. Hard-delete fully-revoked sessions older than the
        //    refresh-token lifetime. Active and rotated rows stay so
        //    reuse detection keeps its history during the refresh
        //    window; only rows that already lost their grace period
        //    get cleaned.
        if let Err(e) = sqlx::query(
            "DELETE FROM user_sessions
              WHERE revoked_at IS NOT NULL
                AND revoked_at < NOW() - INTERVAL '30 days'"
        ).execute(&state.db.pool).await {
            warn!("Session GC error: {e}");
        }

        // 7. Purge age-verification PII blobs past their grace window.
        //    Decided cases keep their metadata (for the audit trail)
        //    but their on-disk biometric + ID blobs are scrubbed:
        //      * approved → 7-day grace (decision is settled fast)
        //      * rejected → 30-day grace (user appeal window)
        //      * withdrawn → already purged at withdraw time
        //    Once the blob files are gone, blobs_purged_at is set and
        //    the paths are nulled so a re-run is a no-op.
        let due: Vec<(uuid::Uuid, Option<String>, Option<String>, String)> = sqlx::query_as(
            "SELECT id, face_scan_path, id_document_path, status
               FROM age_verification_cases
              WHERE blobs_purged_at IS NULL
                AND reviewed_at IS NOT NULL
                AND ((status = 'approved' AND reviewed_at < NOW() - INTERVAL '7 days')
                  OR (status = 'rejected' AND reviewed_at < NOW() - INTERVAL '30 days'))
              LIMIT 200"
        ).fetch_all(&state.db.pool).await.unwrap_or_default();
        for (cid, fp, ip, _st) in due {
            if let Some(p) = &fp { let _ = crate::services::pii_vault::purge_blob(p).await; }
            if let Some(p) = &ip { let _ = crate::services::pii_vault::purge_blob(p).await; }
            if let Err(e) = sqlx::query(
                "UPDATE age_verification_cases
                    SET face_scan_path = NULL,
                        id_document_path = NULL,
                        blobs_purged_at = NOW()
                  WHERE id = $1"
            ).bind(cid).execute(&state.db.pool).await {
                warn!("Age-verify blob purge update error: {e}");
            }
        }

        // 8. Prune stale E2EE devices. A device's last_seen_at is
        //    bumped on every prekey provision/replenish (i.e. whenever
        //    the user opens messaging on it). A device untouched for
        //    90 days is treated as gone: drop its prekeys and the
        //    device row so senders stop wasting a multi-device fan-out
        //    slot encrypting to a browser that will never read it.
        //    The prekeys are keyed by (user_id, device_id) (no FK to
        //    user_devices), so delete both explicitly.
        let stale: Vec<(uuid::Uuid, String)> = sqlx::query_as(
            "SELECT user_id, device_id FROM user_devices
              WHERE last_seen_at < NOW() - INTERVAL '90 days'"
        ).fetch_all(&state.db.pool).await.unwrap_or_default();
        for (uid, dev) in stale {
            let _ = sqlx::query("DELETE FROM one_time_prekeys WHERE user_id = $1 AND device_id = $2")
                .bind(uid).bind(&dev).execute(&state.db.pool).await;
            let _ = sqlx::query("DELETE FROM signed_prekeys WHERE user_id = $1 AND device_id = $2")
                .bind(uid).bind(&dev).execute(&state.db.pool).await;
            if let Err(e) = sqlx::query("DELETE FROM user_devices WHERE user_id = $1 AND device_id = $2")
                .bind(uid).bind(&dev).execute(&state.db.pool).await {
                warn!("Stale-device prune error: {e}");
            }
        }
    }
}

/// Materialise due rows from `scheduled_posts` into `posts`. Runs once
/// a minute — fine for "schedule for 14:32" granularity. The INSERT
/// uses the scheduled row's own `publish_at` as the base for
/// `expires_at`, so a post scheduled for next Tuesday at 9am vanishes
/// next Wednesday at 9am, not 24h after the row was created. Permanent
/// posts get the 100-year horizon used elsewhere.
pub async fn start_scheduled_publish_job(state: AppState) {
    let mut ticker = interval(Duration::from_secs(60));
    loop {
        ticker.tick().await;
        // Two-step: select + delete + insert, all in one tx so we don't
        // double-publish if the worker crashes mid-loop and restarts.
        let mut tx = match state.db.pool.begin().await {
            Ok(t) => t,
            Err(e) => { error!("scheduled-publish: begin tx: {e}"); continue; }
        };

        let due: Vec<DueScheduledPost> = match sqlx::query_as(
            "SELECT id, author_id, content, media_url, is_adult, is_nft,
                    nft_price_yeet::float8 AS nft_price_yeet,
                    is_permanent,
                    ppv_price_yeet::float8 AS ppv_price_yeet,
                    publish_at
               FROM scheduled_posts
              WHERE publish_at <= NOW()
              ORDER BY publish_at ASC
              LIMIT 500
              FOR UPDATE SKIP LOCKED"
        ).fetch_all(&mut *tx).await {
            Ok(v) => v,
            Err(e) => { error!("scheduled-publish: select: {e}"); let _ = tx.rollback().await; continue; }
        };

        if due.is_empty() {
            let _ = tx.rollback().await;
            continue;
        }

        let mut published = 0u64;
        for row in &due {
            let expires_at = if row.is_permanent || row.is_nft {
                row.publish_at + chrono::Duration::hours(24 * 365 * 100)
            } else {
                row.publish_at + chrono::Duration::hours(24)
            };
            let media_arr: Vec<String> = row.media_url.iter().cloned().collect();
            let insert = sqlx::query(
                "INSERT INTO posts
                   (author_id, content, media_urls, media_url, expires_at, is_adult,
                    is_nft, nft_price_yeet, is_permanent, ppv_price_yeet, created_at)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)"
            )
            .bind(row.author_id).bind(&row.content).bind(&media_arr)
            .bind(row.media_url.as_deref())
            .bind(expires_at)
            .bind(row.is_adult).bind(row.is_nft).bind(row.nft_price_yeet)
            // Store computed permanence (matches expires_at above) so an NFT
            // scheduled post materialises into the permanent list, not just
            // the 24h feed.
            .bind(row.is_permanent || row.is_nft).bind(row.ppv_price_yeet)
            .bind(row.publish_at)
            .execute(&mut *tx).await;
            if let Err(e) = insert {
                warn!("scheduled-publish: insert post {}: {e}", row.id);
                continue;
            }
            if let Err(e) = sqlx::query("DELETE FROM scheduled_posts WHERE id = $1")
                .bind(row.id).execute(&mut *tx).await
            {
                warn!("scheduled-publish: delete sched {}: {e}", row.id);
                continue;
            }
            published += 1;
        }

        if let Err(e) = tx.commit().await {
            error!("scheduled-publish: commit: {e}");
        } else if published > 0 {
            info!("Scheduled-publish: materialised {published} posts.");
        }
    }
}

/// Janitor for `lives`. Closes broadcasts that the host never properly
/// ended (status='live' with no events for 6h — probably a client
/// crash) and marks scheduled lives that were never started as
/// 'cancelled' once they're 2h past their start slot. Also prunes
/// fully-ended lives older than 30 days so the table doesn't grow
/// unbounded.
pub async fn start_lives_sweep_job(state: AppState) {
    let mut ticker = interval(Duration::from_secs(600)); // 10 min
    loop {
        ticker.tick().await;

        if let Err(e) = sqlx::query(
            "UPDATE lives
                SET status = 'ended', ended_at = NOW()
              WHERE status = 'live'
                AND started_at < NOW() - INTERVAL '6 hours'"
        ).execute(&state.db.pool).await {
            warn!("lives sweep (stuck-live): {e}");
        }

        // Cancelling expired-scheduled lives also needs to refund any
        // promo the host pre-paid for. Do both in one tx so we never
        // leak YEET if the second statement fails mid-flight.
        match state.db.pool.begin().await {
            Ok(mut tx) => {
                let scan = sqlx::query_scalar::<_, uuid::Uuid>(
                    "SELECT id FROM lives
                      WHERE status = 'scheduled'
                        AND scheduled_for IS NOT NULL
                        AND scheduled_for < NOW() - INTERVAL '2 hours'
                      FOR UPDATE"
                ).fetch_all(&mut *tx).await;
                let expired_ids: Vec<uuid::Uuid> = match scan {
                    Ok(v) => v,
                    Err(e) => {
                        warn!("lives sweep (expired-scheduled scan): {e}");
                        let _ = tx.rollback().await;
                        continue;
                    }
                };
                for live_id in &expired_ids {
                    if let Err(e) = sqlx::query("UPDATE lives SET status = 'cancelled' WHERE id = $1")
                        .bind(live_id).execute(&mut *tx).await {
                        warn!("lives sweep (cancel {live_id}): {e}");
                        continue;
                    }
                    // Inline refund — same logic as the API path, but
                    // we can't call refund_promotion_in_tx from here
                    // without leaking a circular import, so we duplicate
                    // the SQL. Keep it identical to api::lives.
                    let promo: Option<(uuid::Uuid, uuid::Uuid, f64)> = sqlx::query_as(
                        "SELECT id, user_id, cost_yeet::float8
                           FROM live_promotions
                          WHERE live_id = $1 AND status = 'booked'
                          FOR UPDATE"
                    ).bind(live_id).fetch_optional(&mut *tx).await.ok().flatten();
                    if let Some((promo_id, user_id, cost)) = promo {
                        let _ = sqlx::query("UPDATE users SET yeet_token_balance = yeet_token_balance + $1 WHERE id = $2")
                            .bind(cost).bind(user_id).execute(&mut *tx).await;
                        let _ = sqlx::query("UPDATE fee_wallet_balance SET total_yeet = total_yeet - $1 WHERE id = 1")
                            .bind(cost).execute(&mut *tx).await;
                        let _ = sqlx::query(
                            "INSERT INTO fee_ledger (source_type, source_id, gross_amount, fee_amount, creator_amount)
                             VALUES ('live_promo_refund', $1, $2, $2, 0)"
                        ).bind(promo_id).bind(-cost).execute(&mut *tx).await;
                        let _ = sqlx::query("UPDATE live_promotions SET status = 'refunded', refunded_at = NOW() WHERE id = $1")
                            .bind(promo_id).execute(&mut *tx).await;
                    }
                }
                if let Err(e) = tx.commit().await {
                    warn!("lives sweep (expired-scheduled commit): {e}");
                }
            }
            Err(e) => warn!("lives sweep (expired-scheduled tx): {e}"),
        }

        if let Err(e) = sqlx::query(
            "DELETE FROM lives
              WHERE status IN ('ended','cancelled')
                AND COALESCE(ended_at, created_at) < NOW() - INTERVAL '30 days'"
        ).execute(&state.db.pool).await {
            warn!("lives sweep (old-rows): {e}");
        }
    }
}
