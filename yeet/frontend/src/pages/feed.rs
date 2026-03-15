use leptos::*;
use leptos_router::use_query_map;
use shared::Post;
use crate::components::{composer::Composer, post_card::PostCard};
use crate::stores::AuthStore;

#[component]
pub fn Feed() -> impl IntoView {
    let query = use_query_map();
    let auth = use_context::<RwSignal<AuthStore>>().expect("AuthStore missing");
    let api = option_env!("API_URL").unwrap_or("http://localhost:8080/api/v1");

    // Reactive feed mode from URL query param
    let mode = move || query.with(|q| q.get("mode").cloned().unwrap_or("global".into()));
    let show18 = move || query.with(|q| q.get("show18").map(|v| v == "true").unwrap_or(false));

    // Load posts reactively on mode change
    let posts = create_resource(
        move || (mode(), show18()),
        move |(m, s18)| async move {
            let url = format!("{}/feed?mode={}&show_18_plus={}&limit=30", api, m, s18);
            let req = gloo_net::http::Request::get(&url);
            let req = if let Some(token) = auth.get_untracked().token {
                req.header("Authorization", &format!("Bearer {}", token))
            } else { req };

            match req.send().await {
                Ok(resp) => {
                    let data: serde_json::Value = resp.json().await.unwrap_or_default();
                    let posts: Vec<Post> = serde_json::from_value(
                        data["data"].clone()
                    ).unwrap_or_default();
                    posts
                }
                Err(_) => vec![],
            }
        }
    );

    view! {
        <div class="feed-page">
            // Composer only for logged-in users
            <Composer/>

            // Feed mode tabs
            <div class="feed-tabs">
                <a href="/?mode=global"
                    class="feed-tab"
                    class:active=move || mode() == "global">
                    "🌍 Global"
                </a>
                <a href="/?mode=following"
                    class="feed-tab"
                    class:active=move || mode() == "following">
                    "👥 Following"
                </a>
                <a href="/?mode=subscriptions"
                    class="feed-tab"
                    class:active=move || mode() == "subscriptions">
                    "⭐ Subscriptions"
                </a>
            </div>

            // Posts
            <Suspense fallback=move || view! {
                <div class="loading-posts">
                    <div class="spinner"/>
                    <p>"Loading Yeets..."</p>
                </div>
            }>
                {move || posts.get().map(|ps| {
                    if ps.is_empty() {
                        view! {
                            <div class="empty-feed">
                                <p>"No Yeets yet. Be the first! 🚀"</p>
                            </div>
                        }.into_view()
                    } else {
                        ps.into_iter()
                            .map(|p| view! { <PostCard post=p/> })
                            .collect_view()
                    }
                })}
            </Suspense>
        </div>
    }
}
