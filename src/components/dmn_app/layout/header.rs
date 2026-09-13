use gloo_storage::{LocalStorage, Storage};
use leptos::leptos_dom::helpers::window_event_listener;
use leptos::prelude::*;
use leptos_router::components::A;

use crate::components::dmn_app::context::use_app_context;
use crate::components::workspace::RunStatus;
use crate::state::Theme;

#[component]
pub fn Header(
    status: ReadSignal<RunStatus>,
    user_id: ReadSignal<String>,
    user_role: ReadSignal<String>,
    validation_running: ReadSignal<bool>,
    xml: ReadSignal<Option<String>>,
    has_unsaved_changes: Signal<bool>,
    on_validate: Callback<()>,
) -> impl IntoView {
    let ctx = use_app_context();
    let app_state = expect_context::<crate::state::AppState>();

    let logout = move |_| {
        if app_state.ui.workspace_dirty.get_untracked()
            && !web_sys::window().is_some_and(|window| {
                window
                    .confirm_with_message(
                        "当前工作台有未确认的需求或模型变更，退出将放弃本地内容。确定退出吗？",
                    )
                    .unwrap_or(false)
            })
        {
            return;
        }
        let _ = LocalStorage::delete("auth_token");
        let _ = web_sys::window().and_then(|window| window.location().set_href("/login").ok());
    };

    let beforeunload_listener = window_event_listener(leptos::ev::beforeunload, move |event| {
        if has_unsaved_changes.get_untracked() {
            event.set_return_value("");
        }
    });
    on_cleanup(move || beforeunload_listener.remove());

    let confirm_navigation = move |event: leptos::ev::MouseEvent| {
        if has_unsaved_changes.get_untracked() {
            let confirmed = web_sys::window().is_some_and(|window| {
                window
                    .confirm_with_message(
                        "当前工作台有未确认的需求或模型变更，离开将放弃本地内容。确定离开吗？",
                    )
                    .unwrap_or(false)
            });
            if !confirmed {
                event.prevent_default();
            }
        }
    };

    view! {
        <header class="topbar">
            <div class="brand-lockup">
                <div class="brand-mark">
                    <img src="/images/fuxi.png" alt="伏羲" />
                </div>
                <div>
                    <p class="eyebrow">"DMN DECISION WORKSPACE"</p>
                    <h1>"伏羲智能决策建模工作台"</h1>
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
                    on:click=move |_| app_state.set_theme(ctx.theme.get_untracked().toggle())
                >
                    <span class="theme-toggle-icon" aria-hidden="true">
                        {move || if ctx.theme.get() == Theme::Dark { "☼" } else { "☾" }}
                    </span>
                    <span>{move || if ctx.theme.get() == Theme::Dark { "浅色" } else { "深色" }}</span>
                </button>
                <details class="user-menu">
                    <summary class="user-menu-trigger" aria-label="打开用户菜单">
                        <span class="user-avatar" aria-hidden="true">
                            {move || user_id.get().chars().next().unwrap_or('用').to_string()}
                        </span>
                        <span class="user-menu-name">
                            {move || if user_id.get().is_empty() { "当前用户".to_string() } else { user_id.get() }}
                        </span>
                        <span class="user-menu-chevron" aria-hidden="true">"⌄"</span>
                    </summary>
                    <div class="user-menu-popover">
                        <div class="user-menu-heading">
                            <strong>{move || if user_id.get().is_empty() { "当前用户".to_string() } else { user_id.get() }}</strong>
                            <small>{move || {
                                let role = user_role.get();
                                match role.as_str() {
                                    "admin" => "管理员".to_string(),
                                    "" => "工作台成员".to_string(),
                                    role => role.to_string(),
                                }
                            }}</small>
                        </div>
                        <div class="user-menu-divider"></div>
                        <A href="/projects" class:user-menu-item=true on:click=confirm_navigation>"项目管理"</A>
                        <Show when=move || user_role.get() == "admin">
                            <A href="/admin" class:user-menu-item=true on:click=confirm_navigation>"用户管理"</A>
                        </Show>
                        <button class="user-menu-item user-menu-danger" on:click=logout>
                            "退出登录"
                        </button>
                    </div>
                </details>
                <button
                    class="button button-primary"
                    on:click=move |_| on_validate.run(())
                    disabled=move || xml.get().is_none() || validation_running.get()
                >
                    {move || if validation_running.get() { "核验中..." } else { "人工核验" }}
                </button>
            </div>
        </header>
    }
}
