use leptos::*;
use leptos_router::use_params_map;
use shared::{Post, Comment};
use uuid::Uuid;
use crate::components::post_card::PostCard;
use crate::stores::AuthStore;

#[component]
pub fn PostDetail() -> impl IntoView {
    let params = use_params_map();
    let auth = use_context::<RwSignal<AuthStore>>().expect("AuthStore missing");
    let api = option_env!("API_URL").unwrap_or("http://localhost:8080/api/v1");

    let post_id = move || params.with(|p|
        p.get("id").and_then(|s| s.parse::<Uuid>().ok())
    );

    let post = create_resource(post_id, move |id| async move {
        let Some(id) = id else { return None };
        let url = format!("{}/posts/{}", api, id);
        match gloo_net::http::Request::get(&url).send().await {
            Ok(r) => {
                let data: serde_json::Value = r.json().await.unwrap_or_default();
                serde_json::from_value::<Post>(data["data"].clone()).ok()
            }
            Err(_) => None,
        }
    });

    let comments = create_resource(post_id, move |id| async move {
        let Some(id) = id else { return vec![] };
        let url = format!("{}/posts/{}/comments", api, id);
        match gloo_net::http::Request::get(&url).send().await {
            Ok(r) => {
                let data: serde_json::Value = r.json().await.unwrap_or_default();
                serde_json::from_value::<Vec<Comment>>(data["data"].clone())
                    .unwrap_or_default()
            }
            Err(_) => vec![],
        }
    });

    let (new_comment, set_new_comment) = create_signal(String::new());
    let (posting, set_posting) = create_signal(false);

    let handle_comment = move |_| {
        let token = match auth.get().token.clone() { Some(t) => t, None => return };
        let text = new_comment.get_untracked();
        if text.trim().is_empty() { return; }
        let Some(id) = post_id() else { return };

        set_posting(true);
        wasm_bindgen_futures::spawn_local(async move {
            let body = serde_json::json!({ "content": text });
            if let Ok(resp) = gloo_net::http::Request::post(
                &format!("{}/posts/{}/comments", api, id)
            )
            .header("Authorization", &format!("Bearer {}", token))
            .json(&body).unwrap()
            .send().await
            {
                if resp.ok() {
                    set_new_comment(String::new());
                    comments.refetch();
                }
            }
            set_posting(false);
        });
    };

    view! {
        <div class="post-detail">
            <Suspense fallback=move || view! { <div class="loading">"Loading..."</div> }>
                {move || post.get().flatten().map(|p| view! { <PostCard post=p/> })}
            </Suspense>

            // Comment input
            {move || auth.get().is_authenticated().then(|| view! {
                <div class="comment-input">
                    <textarea
                        placeholder="Add a comment..."
                        prop:value=new_comment
                        on:input=move |e| set_new_comment(event_target_value(&e))
                    />
                    <button on:click=handle_comment disabled=posting>
                        {move || if posting.get() { "Posting..." } else { "Reply" }}
                    </button>
                </div>
            })}

            // Comments list
            <div class="comments-list">
                <Suspense fallback=move || view! { <p>"Loading comments..."</p> }>
                    {move || comments.get().map(|cs| cs.into_iter().map(|c| view! {
                        <div class="comment">
                            <strong>"@"{c.author_username}</strong>
                            <p>{c.content}</p>
                        </div>
                    }).collect_view())}
                </Suspense>
            </div>
        </div>
    }
}
