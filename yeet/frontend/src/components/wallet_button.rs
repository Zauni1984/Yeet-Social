use leptos::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use crate::stores::AuthStore;

/// Calls window.ethereum.request({ method: "eth_requestAccounts" })
#[wasm_bindgen(inline_js = r#"
export async function connect_wallet() {
    if (!window.ethereum) throw new Error("MetaMask not installed");
    const accounts = await window.ethereum.request({ method: "eth_requestAccounts" });
    return accounts[0];
}

export async function sign_message(account, message) {
    return await window.ethereum.request({
        method: "personal_sign",
        params: [message, account],
    });
}

export function get_connected_account() {
    if (!window.ethereum || !window.ethereum.selectedAddress) return null;
    return window.ethereum.selectedAddress;
}
"#)]
extern "C" {
    #[wasm_bindgen(catch)]
    async fn connect_wallet() -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch)]
    async fn sign_message(account: &str, message: &str) -> Result<JsValue, JsValue>;

    fn get_connected_account() -> Option<String>;
}

#[component]
pub fn WalletButton() -> impl IntoView {
    let auth = use_context::<RwSignal<AuthStore>>().expect("AuthStore missing");
    let (connecting, set_connecting) = create_signal(false);
    let (error, set_error) = create_signal::<Option<String>>(None);

    // Shorten wallet address for display: 0x1234...abcd
    let wallet_short = move || {
        auth.get().wallet_address.as_ref().map(|w| {
            format!("{}...{}", &w[..6], &w[w.len()-4..])
        })
    };

    let handle_connect = move |_| {
        set_connecting(true);
        set_error(None);

        spawn_local(async move {
            match do_wallet_login().await {
                Ok((token, user_id, wallet)) => {
                    auth.update(|a| {
                        a.token = Some(token);
                        a.user_id = Some(user_id);
                        a.wallet_address = Some(wallet);
                        a.save();
                    });
                }
                Err(e) => set_error(Some(e)),
            }
            set_connecting(false);
        });
    };

    let handle_disconnect = move |_| {
        auth.update(|a| a.logout());
    };

    view! {
        <div class="wallet-btn-wrapper">
            {move || match wallet_short() {
                Some(short) => view! {
                    <div class="wallet-connected">
                        <span class="wallet-dot connected"></span>
                        <span class="wallet-address">{short}</span>
                        <button class="btn-disconnect" on:click=handle_disconnect>
                            "Disconnect"
                        </button>
                    </div>
                }.into_view(),
                None => view! {
                    <button
                        class="btn-connect-wallet"
                        on:click=handle_connect
                        disabled=connecting
                    >
                        {move || if connecting.get() {
                            "Connecting...".to_string()
                        } else {
                            "🦊 Connect Wallet".to_string()
                        }}
                    </button>
                }.into_view(),
            }}
            {move || error.get().map(|e| view! {
                <span class="wallet-error">{e}</span>
            })}
        </div>
    }
}

/// Full MetaMask login flow:
/// 1. Request wallet accounts
/// 2. Fetch nonce from backend
/// 3. Sign nonce message
/// 4. Send signature to backend for JWT
async fn do_wallet_login() -> Result<(String, uuid::Uuid, String), String> {
    let api = option_env!("API_URL").unwrap_or("http://localhost:8080/api/v1");

    // Step 1: Connect MetaMask
    let wallet = connect_wallet().await
        .map_err(|e| format!("Wallet error: {:?}", e))?
        .as_string()
        .ok_or("No wallet address returned")?;

    // Step 2: Get nonce from backend
    let nonce_url = format!("{}/auth/nonce/{}", api, wallet);
    let resp = gloo_net::http::Request::get(&nonce_url)
        .send().await
        .map_err(|e| format!("Network error: {e}"))?;

    let nonce_data: serde_json::Value = resp.json().await
        .map_err(|_| "Invalid nonce response")?;
    let message = nonce_data["data"]["message"]
        .as_str()
        .ok_or("No message in nonce response")?
        .to_string();

    // Step 3: Sign with MetaMask
    let signature = sign_message(&wallet, &message).await
        .map_err(|_| "User rejected signature")?
        .as_string()
        .ok_or("No signature returned")?;

    // Step 4: Login
    let login_body = serde_json::json!({
        "wallet_address": wallet,
        "signature": signature,
        "message": message,
    });

    let login_resp = gloo_net::http::Request::post(&format!("{}/auth/wallet-login", api))
        .json(&login_body)
        .map_err(|_| "Failed to build request")?
        .send().await
        .map_err(|e| format!("Login error: {e}"))?;

    let login_data: serde_json::Value = login_resp.json().await
        .map_err(|_| "Invalid login response")?;

    if login_data["success"].as_bool() != Some(true) {
        return Err(login_data["error"].as_str()
            .unwrap_or("Login failed").to_string());
    }

    let token = login_data["data"]["token"].as_str()
        .ok_or("No token")?.to_string();
    let user_id = login_data["data"]["user_id"].as_str()
        .ok_or("No user_id")?
        .parse::<uuid::Uuid>()
        .map_err(|_| "Invalid user_id")?;

    Ok((token, user_id, wallet))
}
