use leptos::*;
use leptos_router::use_params_map;
use shared::{Post, User};
use crate::components::post_card::PostCard;
use crate::stores::AuthStore;

#[component]
pub fn Profile() -> impl IntoView {
    let params = use_params_map();
    let auth = use_context::<RwSignal<AuthStore>>().expect("AuthStore missing");
    let api = option_env!("API_URL").unwrap_or("http://localhost:8080/api/v1");

    let username = move || params.with(|p| p.get("username").cloned().unwrap_or_default());

    let user = create_resource(username, move |uname| async move {
        let url = format!("{}/users/{}", api, uname);
        match gloo_net::http::Request::get(&url).send().await {
            Ok(r) => {
                let data: serde_json::Value = r.json().await.unwrap_or_default();
                serde_json::from_value::<User>(data["data"].clone()).ok()
            }
            Err(_) => None,
        }
    });

    let is_own_profile = move || {
        auth.get().username.as_deref() == Some(&username())
    };

    view! {
        <div class="profile-page">
            <Suspense fallback=move || view! { <div class="loading">"Loading profile..."</div> }>
                {move || user.get().flatten().map(|u| {
                    let wallet_short = u.wallet_address.as_ref().map(|w|
                        format!("{}...{}", &w[..6], &w[w.len()-4..])
                    );
                    view! {
                        <div class="profile-header">
                            <div class="profile-avatar">
                                {u.username.chars().next().unwrap_or('?')
                                    .to_uppercase().to_string()}
                            </div>
                            <div class="profile-info">
                                <h2 class="profile-name">{u.display_name.clone()}</h2>
                                <span class="profile-username">"@"{u.username.clone()}</span>
                                {u.bio.map(|b| view! { <p class="profile-bio">{b}</p> })}
                                {wallet_short.map(|w| view! {
                                    <span class="profile-wallet" title="BSC Wallet">
                                        "💎 "{w}
                                    </span>
                                })}
                                <div class="profile-tokens">
                                    "⚡ "{format!("{:.1}", u.yeet_token_balance)}" YEET"
                                </div>
                            </div>
                            {(!is_own_profile()).then(|| view! {
                                <button class="btn-follow">"Follow"</button>
                            })}
                        </div>
                    }
                })}
            </Suspense>
        </div>
    }
}
