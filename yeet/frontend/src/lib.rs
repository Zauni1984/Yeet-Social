mod components;
mod pages;
mod stores;

use leptos::*;
use leptos_router::*;
use leptos_meta::*;

use pages::{Feed, Profile, Login, PostDetail};
use stores::AuthStore;

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    // Global auth state
    let auth = create_rw_signal(AuthStore::load_from_storage());
    provide_context(auth);

    view! {
        <Stylesheet href="/pkg/yeet.css"/>
        <Title text="Yeet — Decentralized Social"/>
        <Meta name="viewport" content="width=device-width, initial-scale=1"/>

        <Router>
            <components::layout::Layout>
                <Routes>
                    <Route path="/"        view=Feed/>
                    <Route path="/login"   view=Login/>
                    <Route path="/:username" view=Profile/>
                    <Route path="/post/:id"  view=PostDetail/>
                </Routes>
            </components::layout::Layout>
        </Router>
    }
}

#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    tracing_wasm::set_as_global_default();
    mount_to_body(App);
}
