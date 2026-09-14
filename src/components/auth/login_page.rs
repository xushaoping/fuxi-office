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
    let captcha_id = RwSignal::new(String::new());
    let captcha_code = RwSignal::new(String::new());
    let captcha_image = RwSignal::new(String::new());
    let captcha_loading = RwSignal::new(false);
    let captcha_error = RwSignal::new(None::<String>);
    let app_state = expect_context::<AppState>();

    let refresh_captcha = move || {
        let captcha_id = captcha_id;
        let captcha_image = captcha_image;
        let captcha_loading = captcha_loading;
        let captcha_error = captcha_error;
        captcha_code.set(String::new());
        if captcha_loading.get() {
            return;
        }
        captcha_loading.set(true);
        captcha_error.set(None);
        spawn_local(async move {
            match api::captcha().await {
                Ok(data) => {
                    captcha_id.set(data.captcha_id);
                    captcha_image.set(data.image);
                    captcha_loading.set(false);
                }
                Err(error) => {
                    warn!("验证码加载失败：{error}");
                    captcha_id.set(String::new());
                    captcha_image.set(String::new());
                    captcha_error.set(Some(error));
                    captcha_loading.set(false);
                }
            }
        });
    };
    refresh_captcha();

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default(); // 阻止表单默认提交导致页面刷新

        if is_loading.get() {
            return;
        } // 防止重复提交
        is_loading.set(true);
        error_msg.set(None);

        let username = username.get();
        let password = password.get();
        let captcha_id_value = captcha_id.get();
        let captcha_code_value = captcha_code.get();
        let navigate = navigate.clone();
        let refresh_captcha = refresh_captcha.clone();

        spawn_local(async move {
            log!("开始发送登录请求...");
            match api::login(&username, &password, &captcha_id_value, &captcha_code_value).await {
                Some(t) => {
                    LocalStorage::set(TOKEN_KEY, t.clone()).ok();
                    token.set(Some(t));
                    navigate("/", Default::default());
                }
                None => {
                    warn!("登录失败：用户名或密码错误");
                    error_msg.set(Some("验证码错误或用户名、密码不正确，请重试".to_string()));
                    refresh_captcha();
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
                <div class="form-group">
                    <label for="login-captcha">"验证码"</label>
                    <div class="auth-captcha-field">
                        <input
                            id="login-captcha"
                            type="text"
                            class="input"
                            placeholder="输入图片中的字符"
                            autocomplete="off"
                            name="captchaCode"
                            maxlength="5"
                            required=true
                            bind:value=captcha_code
                            disabled=move || is_loading.get()
                        />
                        <button
                            type="button"
                            class="auth-captcha-refresh"
                            aria-label="刷新验证码"
                            title="看不清？点击刷新"
                            on:click=move |_| refresh_captcha()
                            disabled=move || is_loading.get() || captcha_loading.get()
                        >
                            {move || if captcha_loading.get() { "加载中..." } else if captcha_error.get().is_some() { "重新加载" } else { "换一张" }}
                        </button>
                    </div>
                    {move || (!captcha_image.get().is_empty()).then(|| view! {
                        <img class="auth-captcha-image" src=captcha_image alt="验证码图片，点击换一张" on:click=move |_| refresh_captcha() />
                    })}
                    {move || captcha_error.get().map(|error| view! {
                        <span class="auth-captcha-error" role="status">{error}</span>
                    })}
                </div>
                <button
                    type="submit"
                    class="button button-primary"
                    style="width: 100%;"
                    disabled=move || is_loading.get() || captcha_loading.get() || captcha_id.get().is_empty()
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
