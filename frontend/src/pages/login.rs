use leptos::*;
use crate::components::wallet_button::WalletButton;

#[component]
pub fn Login() -> impl IntoView {
    view! {
        <div class="login-page">
            <div class="login-card">
                <h1>"Welcome to Yeet"</h1>
                <p class="login-subtitle">
                    "Decentralized social media on Binance Smart Chain."
                </p>
                <div class="login-features">
                    <div class="feature">"🌍 Global & Following feed"</div>
                    <div class="feature">"💰 Earn YEET tokens for activity"</div>
                    <div class="feature">"🔒 Own your content as NFT"</div>
                    <div class="feature">"📡 Connect any web board"</div>
                </div>
                <WalletButton/>
                <p class="login-note">
                    "No password needed — your BSC wallet is your identity."
                </p>
            </div>
        </div>
    }
}
