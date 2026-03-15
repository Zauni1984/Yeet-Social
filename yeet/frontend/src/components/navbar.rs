use leptos::*;
use leptos_router::*;
use crate::stores::AuthStore;
use crate::components::wallet_button::WalletButton;

#[component]
pub fn Navbar() -> impl IntoView {
    let auth = use_context::<RwSignal<AuthStore>>().expect("AuthStore missing");

    view! {
        <nav class="navbar">
            <div class="navbar-brand">
                <A href="/">
                    <span class="brand-logo">"Y"</span>
                    <span class="brand-name">"eet"</span>
                </A>
            </div>

            // Feed mode tabs
            <div class="navbar-tabs">
                <A href="/?mode=global"      class="tab">"🌍 Global"</A>
                <A href="/?mode=following"   class="tab">"👥 Following"</A>
                <A href="/?mode=subscriptions" class="tab">"⭐ Subscriptions"</A>
            </div>

            <div class="navbar-actions">
                // 18+ toggle
                <label class="toggle-18">
                    <input type="checkbox" id="toggle18plus"/>
                    <span>"18+"</span>
                </label>

                <WalletButton/>

                {move || auth.get().is_authenticated().then(|| view! {
                    <A href="/compose" class="btn-compose">"+ Yeet"</A>
                })}
            </div>
        </nav>
    }
}
