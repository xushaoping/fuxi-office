use gloo_storage::{LocalStorage, Storage};
use js_sys::{Function, Object, Promise, Reflect};
use leptos::logging::{log, warn};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::JsFuture;

use super::auth_guard::{TOKEN_KEY, auth_token};
use crate::api;
use crate::state::AppState;

async fn wait_for_hcaptcha() -> Result<JsValue, String> {
    for _ in 0..50 {
        if let Some(window) = web_sys::window()
            && let Ok(hcaptcha) = Reflect::get(window.as_ref(), &JsValue::from_str("hcaptcha"))
            && !hcaptcha.is_undefined()
            && !hcaptcha.is_null()
        {
            return Ok(hcaptcha);
        }
        let promise = Promise::new(&mut |resolve, _reject| {
            let callback = Closure::once_into_js(move || {
                let _ = resolve.call0(&JsValue::UNDEFINED);
            });
            if let Some(window) = web_sys::window() {
                let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                    callback.unchecked_ref(),
                    100,
                );
            }
        });
        JsFuture::from(promise)
            .await
            .map_err(|_| "hCaptcha 加载失败".to_string())?;
    }
    Err("hCaptcha 加载超时，请刷新页面重试".to_string())
}

fn render_hcaptcha(
    hcaptcha: &JsValue,
    container: &web_sys::HtmlDivElement,
    site_key: &str,
    token: RwSignal<String>,
    error: RwSignal<Option<String>>,
) -> Result<JsValue, String> {
    let render = Reflect::get(hcaptcha, &JsValue::from_str("render"))
        .map_err(|_| "hCaptcha 接口不可用".to_string())?
        .dyn_into::<Function>()
        .map_err(|_| "hCaptcha 渲染接口不可用".to_string())?;
    let options = Object::new();
    Reflect::set(
        &options,
        &JsValue::from_str("sitekey"),
        &JsValue::from_str(site_key),
    )
    .map_err(|_| "hCaptcha 配置无效".to_string())?;
    let callback = Closure::wrap(Box::new(move |value: JsValue| {
        token.set(value.as_string().unwrap_or_default());
        error.set(None);
    }) as Box<dyn FnMut(JsValue)>);
    Reflect::set(
        &options,
        &JsValue::from_str("callback"),
        callback.as_ref().unchecked_ref(),
    )
    .map_err(|_| "hCaptcha 回调配置失败".to_string())?;
    let widget_id = render
        .call2(hcaptcha, container.as_ref(), options.as_ref())
        .map_err(|_| "hCaptcha 渲染失败".to_string())?;
    callback.forget();
    Ok(widget_id)
}

fn reset_hcaptcha(widget_id: Option<JsValue>) {
    let Some(widget_id) = widget_id else {
        return;
    };
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(hcaptcha) = Reflect::get(window.as_ref(), &JsValue::from_str("hcaptcha")) else {
        return;
    };
    let Ok(reset) = Reflect::get(&hcaptcha, &JsValue::from_str("reset")) else {
        return;
    };
    let Ok(reset) = reset.dyn_into::<Function>() else {
        return;
    };
    let _ = reset.call1(&hcaptcha, &widget_id);
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
    let hcaptcha_container = NodeRef::<leptos::html::Div>::new();
    let hcaptcha_site_key = RwSignal::new(String::new());
    let hcaptcha_token = RwSignal::new(String::new());
    let hcaptcha_widget_id = RwSignal::new(None::<JsValue>);
    let hcaptcha_loading = RwSignal::new(true);
    let hcaptcha_error = RwSignal::new(None::<String>);
    let hcaptcha_started = RwSignal::new(false);
    let app_state = expect_context::<AppState>();

    spawn_local({
        let hcaptcha_site_key = hcaptcha_site_key;
        let hcaptcha_error = hcaptcha_error;
        async move {
            match api::hcaptcha_config().await {
                Ok(config) if !config.site_key.trim().is_empty() => {
                    hcaptcha_site_key.set(config.site_key);
                }
                Ok(_) => hcaptcha_error.set(Some("hCaptcha site key 未配置".into())),
                Err(error) => {
                    warn!("hCaptcha 配置加载失败：{error}");
                    hcaptcha_error.set(Some(error));
                    hcaptcha_loading.set(false);
                }
            }
        }
    });

    Effect::new({
        let hcaptcha_container = hcaptcha_container;
        let hcaptcha_site_key = hcaptcha_site_key;
        let hcaptcha_token = hcaptcha_token;
        let hcaptcha_error = hcaptcha_error;
        move |_| {
            let site_key = hcaptcha_site_key.get();
            let Some(container) = hcaptcha_container.get() else {
                return;
            };
            if site_key.is_empty() || hcaptcha_started.get() {
                return;
            }
            hcaptcha_started.set(true);
            let hcaptcha_widget_id = hcaptcha_widget_id;
            let hcaptcha_loading = hcaptcha_loading;
            spawn_local(async move {
                match wait_for_hcaptcha().await.and_then(|hcaptcha| {
                    render_hcaptcha(
                        &hcaptcha,
                        &container,
                        &site_key,
                        hcaptcha_token,
                        hcaptcha_error,
                    )
                }) {
                    Ok(widget_id) => hcaptcha_widget_id.set(Some(widget_id)),
                    Err(error) => hcaptcha_error.set(Some(error)),
                }
                hcaptcha_loading.set(false);
            });
        }
    });

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default(); // 阻止表单默认提交导致页面刷新

        if is_loading.get() {
            return;
        } // 防止重复提交
        is_loading.set(true);
        error_msg.set(None);

        let username = username.get();
        let password = password.get();
        let hcaptcha_token_value = hcaptcha_token.get();
        if hcaptcha_token_value.is_empty() {
            error_msg.set(Some("请先完成 hCaptcha 人机验证".into()));
            is_loading.set(false);
            return;
        }
        let navigate = navigate.clone();
        let hcaptcha_widget_id = hcaptcha_widget_id;
        let hcaptcha_token = hcaptcha_token;

        spawn_local(async move {
            log!("开始发送登录请求...");
            match api::login(&username, &password, &hcaptcha_token_value).await {
                Ok(t) => {
                    LocalStorage::set(TOKEN_KEY, t.clone()).ok();
                    token.set(Some(t));
                    navigate("/", Default::default());
                }
                Err(error) => {
                    warn!("登录失败：{error}");
                    error_msg.set(Some(error));
                    hcaptcha_token.set(String::new());
                    reset_hcaptcha(hcaptcha_widget_id.get());
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
                    <label for="hcaptcha-container">"人机验证"</label>
                    <div id="hcaptcha-container" class="auth-hcaptcha-widget" node_ref=hcaptcha_container></div>
                    {move || hcaptcha_loading.get().then(|| view! {
                        <span class="auth-captcha-status" role="status">"正在加载人机验证..."</span>
                    })}
                    {move || hcaptcha_error.get().map(|error| view! {
                        <span class="auth-captcha-error" role="status">{error}</span>
                    })}
                </div>
                <button
                    type="submit"
                    class="button button-primary"
                    style="width: 100%;"
                    disabled=move || is_loading.get() || hcaptcha_loading.get() || hcaptcha_token.get().is_empty()
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
