use leptos::*;
use shared::PostVisibility;
use crate::stores::AuthStore;

#[component]
pub fn Composer() -> impl IntoView {
    let auth = use_context::<RwSignal<AuthStore>>().expect("AuthStore missing");
    let (content, set_content) = create_signal(String::new());
    let (visibility, set_visibility) = create_signal("public".to_string());
    let (ppv_price, set_ppv_price) = create_signal(0.0_f64);
    let (posting, set_posting) = create_signal(false);
    let (char_count, set_char_count) = create_signal(0_usize);
    let api = option_env!("API_URL").unwrap_or("http://localhost:8080/api/v1");

    const MAX_CHARS: usize = 280;

    let is_ppv = move || visibility.get() == "pay_per_view";

    let handle_post = move |_| {
        let token = match auth.get().token.clone() {
            Some(t) => t,
            None => return,
        };
        let text = content.get_untracked();
        if text.trim().is_empty() { return; }

        set_posting(true);

        wasm_bindgen_futures::spawn_local(async move {
            let body = serde_json::json!({
                "content": text,
                "media_urls": [],
                "visibility": visibility.get_untracked(),
                "pay_per_view_price": if is_ppv() { Some(ppv_price.get_untracked()) } else { None },
            });

            if let Ok(resp) = gloo_net::http::Request::post(&format!("{}/posts", api))
                .header("Authorization", &format!("Bearer {}", token))
                .json(&body).unwrap()
                .send().await
            {
                if resp.ok() {
                    set_content(String::new());
                    set_char_count(0);
                }
            }
            set_posting(false);
        });
    };

    view! {
        <div class="composer" class:hidden=move || !auth.get().is_authenticated()>
            <div class="composer-inner">
                <textarea
                    class="composer-input"
                    placeholder="What's happening? #yeet"
                    maxlength=MAX_CHARS.to_string()
                    prop:value=content
                    on:input=move |e| {
                        let v = event_target_value(&e);
                        set_char_count(v.len());
                        set_content(v);
                    }
                />

                <div class="composer-footer">
                    <div class="composer-options">
                        // Visibility selector
                        <select class="visibility-select"
                            on:change=move |e| set_visibility(event_target_value(&e))>
                            <option value="public">"🌍 Public"</option>
                            <option value="followers_only">"👥 Followers only"</option>
                            <option value="age_restricted">"🔞 18+"</option>
                            <option value="pay_per_view">"💰 Pay-per-view"</option>
                        </select>

                        // PPV price input (only shown when pay_per_view selected)
                        {move || is_ppv().then(|| view! {
                            <div class="ppv-price-input">
                                <input
                                    type="number"
                                    min="0.1" step="0.5"
                                    placeholder="Price in YEET"
                                    on:input=move |e| {
                                        if let Ok(v) = event_target_value(&e).parse::<f64>() {
                                            set_ppv_price(v);
                                        }
                                    }
                                />
                                <span class="ppv-label">"YEET"</span>
                            </div>
                        })}
                    </div>

                    <div class="composer-submit">
                        // Character counter
                        <span class="char-count"
                            class:near-limit=move || char_count.get() > 250
                            class:at-limit=move || char_count.get() >= MAX_CHARS>
                            {move || format!("{}/{}", char_count.get(), MAX_CHARS)}
                        </span>

                        <button
                            class="btn-yeet"
                            on:click=handle_post
                            disabled=move || posting.get() || char_count.get() == 0
                        >
                            {move || if posting.get() { "Posting..." } else { "Yeet 🚀" }}
                        </button>
                    </div>
                </div>
            </div>
        </div>
    }
}
