use leptos::*;
use leptos_router::A;
use shared::{Post, PostSource, PostVisibility};
use crate::components::tip_button::TipButton;
use crate::stores::AuthStore;

/// Format expiry countdown: "23h 14m left" or "NFT (permanent)"
fn format_expiry(post: &Post) -> String {
    if post.is_nft {
        return "🔒 NFT (permanent)".to_string();
    }
    let now = chrono::Utc::now();
    let diff = post.expires_at - now;
    if diff.num_seconds() <= 0 {
        return "Expired".to_string();
    }
    let h = diff.num_hours();
    let m = diff.num_minutes() % 60;
    format!("⏱ {h}h {m}m left")
}

#[component]
pub fn PostCard(post: Post) -> impl IntoView {
    let auth = use_context::<RwSignal<AuthStore>>().expect("AuthStore missing");
    let (like_count, set_like_count) = create_signal(post.like_count);
    let (reshare_count, set_reshare_count) = create_signal(post.reshare_count);
    let post_id = post.id;
    let api = option_env!("API_URL").unwrap_or("http://localhost:8080/api/v1");

    // Source badge for web-board posts
    let source_badge = match &post.source {
        PostSource::WebBoard(domain) => Some(domain.clone()),
        PostSource::Yeet => None,
    };

    // Blur PPV content unless unlocked
    let is_ppv = post.visibility == PostVisibility::PayPerView;
    let (ppv_unlocked, set_ppv_unlocked) = create_signal(!is_ppv);

    let expiry_text = format_expiry(&post);

    // Like action
    let handle_like = move |_| {
        let token = auth.get().token.clone();
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(token) = token {
                let url = format!("{}/posts/{}/like", api, post_id);
                if let Ok(resp) = gloo_net::http::Request::post(&url)
                    .header("Authorization", &format!("Bearer {}", token))
                    .send().await
                {
                    if let Ok(data) = resp.json::<serde_json::Value>().await {
                        if let Some(count) = data["data"].as_i64() {
                            set_like_count(count);
                        }
                    }
                }
            }
        });
    };

    // Reshare action — also resets the 24h timer on the original post
    let handle_reshare = move |_| {
        let token = auth.get().token.clone();
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(token) = token {
                let url = format!("{}/posts/{}/reshare", api, post_id);
                let _ = gloo_net::http::Request::post(&url)
                    .header("Authorization", &format!("Bearer {}", token))
                    .send().await;
                set_reshare_count.update(|c| *c += 1);
            }
        });
    };

    view! {
        <article class="post-card" class:is-nft=post.is_nft>

            // ── Header ──────────────────────────────────────────────────────
            <div class="post-header">
                <A href=format!("/{}", post.author_username) class="post-author">
                    <div class="avatar">
                        {post.author_username.chars().next().unwrap_or('?').to_uppercase().to_string()}
                    </div>
                    <span class="username">"@"{post.author_username.clone()}</span>
                </A>

                {source_badge.map(|domain| view! {
                    <span class="source-badge">"📡 "{domain}</span>
                })}

                <span class="expiry-badge" class:nft-badge=post.is_nft>
                    {expiry_text}
                </span>
            </div>

            // ── Content ─────────────────────────────────────────────────────
            <div class="post-content" class:blurred=move || !ppv_unlocked.get()>
                <p>{post.content.clone()}</p>

                {(!post.media_urls.is_empty()).then(|| view! {
                    <div class="post-media">
                        {post.media_urls.iter().map(|url| view! {
                            <img src=url.clone() class="post-image" loading="lazy"/>
                        }).collect_view()}
                    </div>
                })}
            </div>

            // ── PPV unlock overlay ───────────────────────────────────────────
            {is_ppv.then(move || view! {
                <div class="ppv-overlay" class:hidden=ppv_unlocked>
                    <p class="ppv-price">
                        "🔓 Unlock for "
                        {post.pay_per_view_price.unwrap_or(0.0)}
                        " YEET"
                    </p>
                    <button class="btn-unlock"
                        on:click=move |_| set_ppv_unlocked(true)>
                        "Unlock Post"
                    </button>
                </div>
            })}

            // ── Actions ──────────────────────────────────────────────────────
            <div class="post-actions">
                <button class="action-btn like-btn" on:click=handle_like>
                    "❤️ " {like_count}
                </button>

                <A href=format!("/post/{}", post.id) class="action-btn">
                    "💬 " {post.comment_count}
                </A>

                <button class="action-btn reshare-btn" on:click=handle_reshare
                    title="Reshare resets the 24h timer">
                    "🔁 " {reshare_count}
                </button>

                <TipButton
                    post_id=post.id
                    to_user_id=post.author_id
                    tip_total=post.tip_total
                />

                {post.is_nft.then(|| view! {
                    <a
                        href=format!(
                            "https://bscscan.com/token/{}?a={}",
                            post.nft_contract.clone().unwrap_or_default(),
                            post.nft_token_id.clone().unwrap_or_default()
                        )
                        class="action-btn nft-link"
                        target="_blank"
                    >
                        "🔗 BSCScan"
                    </a>
                })}
            </div>
        </article>
    }
}
