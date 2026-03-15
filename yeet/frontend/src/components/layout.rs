use leptos::*;
use crate::components::navbar::Navbar;

#[component]
pub fn Layout(children: Children) -> impl IntoView {
    view! {
        <div class="yeet-app">
            <Navbar/>
            <main class="yeet-main">
                <div class="yeet-container">
                    {children()}
                </div>
            </main>
        </div>
    }
}
