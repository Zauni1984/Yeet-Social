use leptos::*;
use uuid::Uuid;
use crate::stores::AuthStore;

#[component]
pub fn TipButton(
    post_id: Uuid,
    to_user_id: Uuid,
    tip_total: f64,
) -> impl IntoView {
    let auth = use_context::<RwSignal<AuthStore>>().expect("AuthStore missing");
    let (show_panel, set_show_panel) = create_signal(false);
    let (amount, set_amount) = create_signal(1.0_f64);
    let (currency, set_currency) = create_signal("yeet".to_string());
    let (sending, set_sending) = create_signal(false);
    let (total, set_total) = create_signal(tip_total);
    let api = option_env!("API_URL").unwrap_or("http://localhost:8080/api/v1");

    let handle_tip = move |_| {
        let token = match auth.get().token.clone() {
            Some(t) => t,
            None => return,
        };
        set_sending(true);

        wasm_bindgen_futures::spawn_local(async move {
            let body = serde_json::json!({
                "post_id": post_id,
                "to_user_id": to_user_id,
                "amount": amount.get_untracked(),
                "currency": currency.get_untracked(),
            });

            if let Ok(resp) = gloo_net::http::Request::post(&format!("{}/tips", api))
                .header("Authorization", &format!("Bearer {}", token))
                .json(&body).unwrap()
                .send().await
            {
                if resp.ok() {
                    set_total.update(|t| *t += amount.get_untracked());
                    set_show_panel(false);
                }
            }
            set_sending(false);
        });
    };

    view! {
        <div class="tip-wrapper">
            <button class="action-btn tip-toggle"
                on:click=move |_| set_show_panel.update(|v| *v = !*v)>
                "💰 " {move || format!("{:.1}", total.get())} " YEET"
            </button>

            {move || show_panel.get().then(|| view! {
                <div class="tip-panel">
                    <div class="tip-row">
                        <select
                            class="tip-currency"
                            on:change=move |e| set_currency(event_target_value(&e))
                        >
                            <option value="yeet" selected>"YEET"</option>
                            <option value="bnb">"BNB"</option>
                        </select>

                        <input
                            type="number"
                            class="tip-amount"
                            min="0.1" step="0.5"
                            prop:value=amount
                            on:input=move |e| {
                                if let Ok(v) = event_target_value(&e).parse::<f64>() {
                                    set_amount(v);
                                }
                            }
                        />

                        <button
                            class="btn-send-tip"
                            on:click=handle_tip
                            disabled=sending
                        >
                            {move || if sending.get() { "Sending..." } else { "Send 🚀" }}
                        </button>
                    </div>
                    <p class="tip-note">
                        "10% platform fee · "
                        {move || format!("{:.2}", amount.get() * 0.9)}
                        " goes to creator"
                    </p>
                </div>
            })}
        </div>
    }
}
