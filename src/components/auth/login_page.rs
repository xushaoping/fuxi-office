use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use gloo_storage::{LocalStorage, Storage};
use leptos::logging::{log, warn};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
use sha2::{Digest, Sha256};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use super::auth_guard::{TOKEN_KEY, auth_token};
use crate::api;
use crate::state::AppState;

fn load_captcha(
    captcha_id: RwSignal<String>,
    captcha_image: RwSignal<String>,
    captcha_code: RwSignal<String>,
    captcha_work_nonce: RwSignal<String>,
    captcha_loading: RwSignal<bool>,
    captcha_error: RwSignal<Option<String>>,
) {
    captcha_loading.set(true);
    captcha_error.set(None);
    captcha_id.set(String::new());
    captcha_image.set(String::new());
    captcha_code.set(String::new());
    captcha_work_nonce.set(String::new());
    spawn_local(async move {
        match api::captcha().await {
            Ok(captcha) => {
                let challenge_id = captcha.captcha_id.clone();
                captcha_id.set(challenge_id.clone());
                captcha_image.set(captcha.image);
                let captcha_id_for_expiration = captcha_id;
                let captcha_code_for_expiration = captcha_code;
                let captcha_work_nonce_for_expiration = captcha_work_nonce;
                let captcha_error_for_expiration = captcha_error;
                let challenge_id_for_expiration = challenge_id.clone();
                spawn_local(async move {
                    wait_for_browser(captcha.expires_in.saturating_mul(1000)).await;
                    if captcha_id_for_expiration.get_untracked() == challenge_id_for_expiration {
                        captcha_id_for_expiration.set(String::new());
                        captcha_code_for_expiration.set(String::new());
                        captcha_work_nonce_for_expiration.set(String::new());
                        captcha_error_for_expiration
                            .set(Some("验证码已过期，请刷新后重试".to_string()));
                    }
                });
                match solve_captcha_pow(&captcha.work_challenge, captcha.work_difficulty).await {
                    Ok(nonce) => {
                        if captcha_id.get_untracked() == challenge_id {
                            captcha_work_nonce.set(nonce);
                        }
                    }
                    Err(error) => {
                        warn!("验证码工作量证明失败：{error}");
                        captcha_error.set(Some(error));
                    }
                }
            }
            Err(error) => {
                warn!("验证码加载失败：{error}");
                captcha_error.set(Some(error));
            }
        }
        captcha_loading.set(false);
    });
}

async fn wait_for_browser(milliseconds: u64) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let Some(window) = web_sys::window() else {
            let _ = resolve.call0(&JsValue::NULL);
            return;
        };
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
            resolve.unchecked_ref(),
            milliseconds.min(i32::MAX as u64) as i32,
        );
    });
    let _ = JsFuture::from(promise).await;
}

async fn solve_captcha_pow(challenge: &str, difficulty: u8) -> Result<String, String> {
    let challenge = BASE64
        .decode(challenge)
        .map_err(|_| "验证码挑战数据无效，请刷新页面重试".to_string())?;
    if challenge.len() != 16 || difficulty == 0 || difficulty > 32 {
        return Err("验证码挑战参数无效，请刷新页面重试".to_string());
    }

    for nonce in 0u64.. {
        let mut hasher = Sha256::new();
        hasher.update(&challenge);
        hasher.update(nonce.to_be_bytes());
        let digest = hasher.finalize();
        let full_bytes = (difficulty / 8) as usize;
        let remaining_bits = difficulty % 8;
        if digest[..full_bytes].iter().all(|byte| *byte == 0)
            && (remaining_bits == 0 || digest[full_bytes] >> (8 - remaining_bits) == 0)
        {
            return Ok(nonce.to_string());
        }
        if nonce % 2048 == 2047 {
            wait_for_browser(0).await;
        }
    }

    unreachable!("u64 nonce space exhausted")
}

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
    let captcha_image = RwSignal::new(String::new());
    let captcha_code = RwSignal::new(String::new());
    let captcha_work_nonce = RwSignal::new(String::new());
    let captcha_loading = RwSignal::new(true);
    let captcha_error = RwSignal::new(None::<String>);
    let app_state = expect_context::<AppState>();

    load_captcha(
        captcha_id,
        captcha_image,
        captcha_code,
        captcha_work_nonce,
        captcha_loading,
        captcha_error,
    );

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default(); // 阻止表单默认提交导致页面刷新

        if is_loading.get() {
            return;
        } // 防止重复提交
        is_loading.set(true);
        error_msg.set(None);

        let username = username.get();
        let password = password.get();
        let captcha_code_value = captcha_code.get();
        let captcha_work_nonce_value = captcha_work_nonce.get();
        if captcha_id.get().is_empty()
            || captcha_code_value.trim().is_empty()
            || captcha_work_nonce_value.is_empty()
        {
            error_msg.set(Some("请先完成验证码验证".into()));
            is_loading.set(false);
            return;
        }
        let navigate = navigate.clone();
        let captcha_id_value = captcha_id.get();
        let captcha_code = captcha_code;
        let captcha_work_nonce = captcha_work_nonce;
        let captcha_image = captcha_image;
        let captcha_id = captcha_id;
        let captcha_loading = captcha_loading;
        let captcha_error = captcha_error;

        spawn_local(async move {
            log!("开始发送登录请求...");
            match api::login(
                &username,
                &password,
                &captcha_id_value,
                &captcha_code_value,
                &captcha_work_nonce_value,
            )
            .await
            {
                Ok(t) => {
                    LocalStorage::set(TOKEN_KEY, t.clone()).ok();
                    token.set(Some(t));
                    navigate("/", Default::default());
                }
                Err(error) => {
                    warn!("登录失败：{error}");
                    error_msg.set(Some(error));
                    load_captcha(
                        captcha_id,
                        captcha_image,
                        captcha_code,
                        captcha_work_nonce,
                        captcha_loading,
                        captcha_error,
                    );
                    is_loading.set(false);
                }
            }
        });
    };

    view! {
        <div class=move || format!("auth-shell {}", app_state.ui.theme.get().class())>
            <section class="auth-panel">
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
                    <div class="auth-field-label">
                        <label for="login-username">"用户名"</label>
                    </div>
                    <input
                        id="login-username"
                        type="text"
                        class="input"
                        placeholder="请输入用户名"
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
                    <div class="auth-field-label">
                        <label for="login-password">"密码"</label>
                    </div>
                    <div class="auth-password-field">
                    <input
                        id="login-password"
                        type=move || if password_visible.get() { "text" } else { "password" }
                        class="input"
                        placeholder="请输入密码"
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
                        <label for="login-captcha-code">"登录验证"</label>
                    <div class="auth-captcha-field">
                        <input
                            id="login-captcha-code"
                            type="text"
                            class="input"
                            placeholder="输入算式答案"
                            autocomplete="off"
                            maxlength="3"
                            required=true
                            bind:value=captcha_code
                            disabled=move || is_loading.get() || captcha_loading.get()
                        />
                        <button
                            type="button"
                            class="auth-captcha-refresh"
                            disabled=move || is_loading.get() || captcha_loading.get()
                            on:click=move |_| load_captcha(
                                captcha_id,
                                captcha_image,
                                captcha_code,
                                captcha_work_nonce,
                                captcha_loading,
                                captcha_error,
                            )
                        >"刷新"</button>
                    </div>
                    {move || (!captcha_image.get().is_empty()).then(|| view! {
                        <img
                            class="auth-captcha-image"
                            src=move || captcha_image.get()
                            alt="算术验证码图片，点击刷新"
                            title="点击刷新算术验证码"
                            on:click=move |_| load_captcha(
                                captcha_id,
                                captcha_image,
                                captcha_code,
                                captcha_work_nonce,
                                captcha_loading,
                                captcha_error,
                            )
                        />
                    })}
                    {move || captcha_loading.get().then(|| view! {
                        <span class="auth-captcha-status" role="status">"正在加载并准备本地验证..."</span>
                    })}
                    {move || captcha_error.get().map(|error| view! {
                        <span class="auth-captcha-error" role="status">{error}</span>
                    })}
                </div>
                <button
                    type="submit"
                    class="button button-primary"
                    style="width: 100%;"
                    disabled=move || is_loading.get() || captcha_loading.get() || captcha_code.get().trim().is_empty() || captcha_work_nonce.get().is_empty()
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
            </section>
        </div>
    }
}
