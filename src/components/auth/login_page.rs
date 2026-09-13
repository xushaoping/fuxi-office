use gloo_storage::{LocalStorage, Storage};
use leptos::logging::{log, warn};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;

use super::auth_guard::{TOKEN_KEY, auth_token};
use crate::api;
use crate::state::AppState;

#[component]
pub fn LoginPage() -> impl IntoView {
    let token = auth_token();
    let navigate = use_navigate();
    let username = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let error_msg = RwSignal::new(None::<String>);
    let is_loading = RwSignal::new(false);
    let password_visible = RwSignal::new(false);
    let app_state = expect_context::<AppState>();

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default(); // 阻止表单默认提交导致页面刷新

        if is_loading.get() {
            return;
        } // 防止重复提交
        is_loading.set(true);
        error_msg.set(None);

        let username = username.get();
        let password = password.get();
        let navigate = navigate.clone();

        spawn_local(async move {
            log!("开始发送登录请求...");
            match api::login(&username, &password).await {
                Some(t) => {
                    LocalStorage::set(TOKEN_KEY, t.clone()).ok();
                    token.set(Some(t));
                    navigate("/", Default::default());
                }
                None => {
                    warn!("登录失败：用户名或密码错误");
                    error_msg.set(Some("用户名或密码错误".to_string()));
                    is_loading.set(false);
                }
            }
        });
    };

    view! {
        <div class=move || format!("auth-shell {}", app_state.ui.theme.get().class())>
            <div class="auth-card">
                <div class="auth-brand">
                    <div class="brand-mark">
                        <img src="/images/fuxi.png" alt="" />
                    </div>
                    <div>
                        <p class="eyebrow">"DMN DECISION WORKSPACE"</p>
                        <h1>"伏羲智能决策"</h1>
                    </div>
                </div>
                <div class="auth-heading">
                    <h2 class="title">"欢迎登录"</h2>
                    <p>"进入你的决策建模工作台"</p>
                </div>
            </div>
            <form on:submit=on_submit>
                <div class="form-group">
                    <label for="login-username">"用户名"</label>
                    <input
                        id="login-username"
                        type="text"
                        class="input"
                        placeholder="用户名"
                        autocomplete="username"
                        name="username"
                        required=true
                        aria-describedby=move || error_msg.get().map(|_| "login-error")
                        aria-invalid=move || error_msg.get().is_some()
                        bind:value=username
                        disabled=move || is_loading.get()
                    />
                </div>
                <div class="form-group">
                    <label for="login-password">"密码"</label>
                    <div class="auth-password-field">
                    <input
                        id="login-password"
                        type=move || if password_visible.get() { "text" } else { "password" }
                        class="input"
                        placeholder="密码"
                        autocomplete="current-password"
                        name="password"
                        required=true
                        aria-describedby=move || error_msg.get().map(|_| "login-error")
                        aria-invalid=move || error_msg.get().is_some()
                        bind:value=password
                        disabled=move || is_loading.get()
                    />
                    <button type="button" class="button button-quiet" aria-controls="login-password"
                        aria-label="显示密码" aria-pressed=move || password_visible.get()
                        on:click=move |_| password_visible.update(|visible| *visible = !*visible)
                    >{move || if password_visible.get() { "隐藏" } else { "显示" }}</button>
                    </div>
                </div>
                <button
                    type="submit"
                    class="btn btn-primary"
                    style="width: 100%;"
                    disabled=move || is_loading.get()
                >
                    {move || if is_loading.get() {
                        "处理中..."
                    } else {
                        "登 录"
                    }}
                </button>

                {move || error_msg.get().map(|e| view! {
                    <div id="login-error" class="error-msg" role="alert">{e}</div>
                })}
            </form>
        </div>
    }
}
