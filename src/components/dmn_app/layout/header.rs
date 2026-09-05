use gloo_storage::{LocalStorage, Storage};
use leptos::prelude::*;
use leptos_router::components::A;

use crate::components::dmn_app::context::use_app_context;
use crate::components::dmn_app::types::{RunStatus, Theme};

#[component]
pub fn Header(
    status: ReadSignal<RunStatus>,
    user_role: ReadSignal<String>,
    validation_running: ReadSignal<bool>,
    xml: ReadSignal<Option<String>>,
    on_validate: Callback<()>,
) -> impl IntoView {
    let ctx = use_app_context();

    let logout = move |_| {
        let _ = LocalStorage::delete("auth_token");
        let _ = web_sys::window().and_then(|w| w.location().set_href("/login").ok());
    };

    view! {
        <header class="topbar">
            <div class="brand-lockup">
                <div class="brand-mark">"D"</div>
                <div>
                    <p class="eyebrow">"DMN DECISION WORKSPACE"</p>
                    <h1>"智能决策建模工作台"</h1>
                </div>
            </div>
            <div class="topbar-actions">
                <div class="run-status-wrap">
                    <span class="toolbar-label">"当前状态"</span>
                    <div class=move || format!("run-status {}", status.get().class())>
                        <span class="status-dot"></span>
                        {move || status.get().label()}
                    </div>
                </div>
                <button
                    class="theme-toggle"
                    title=move || ctx.theme.get().toggle_label()
                    aria-label=move || ctx.theme.get().toggle_label()
                    on:click=move |_| ctx.theme.update(|current| *current = match *current {
                        Theme::Dark => Theme::Light,
                        Theme::Light => Theme::Dark,
                    })
                >
                    <span class="theme-toggle-icon" aria-hidden="true">
                        {move || if ctx.theme.get() == Theme::Dark { "☼" } else { "☾" }}
                    </span>
                    <span>{move || if ctx.theme.get() == Theme::Dark { "浅色" } else { "深色" }}</span>
                </button>
                <button
                    class="button button-quiet"
                    title="退出当前账号"
                    on:click=logout
                >"退出登录"</button>
                <Show when=move || user_role.get() == "admin">
                    <A href="/admin" class:button=true class:button-quiet=true>"用户管理"</A>
                </Show>
                <button
                    class="button button-primary"
                    on:click=move |_| on_validate.run(())
                    disabled=move || xml.get().is_none() || validation_running.get()
                >
                    {move || if validation_running.get() { "核验中..." } else { "人工核验 DMN" }}
                </button>
            </div>
        </header>
    }
}
