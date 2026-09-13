use gloo_storage::{LocalStorage, Storage};
use leptos::prelude::*;
use leptos_router::{components::Redirect, hooks::use_navigate};
use wasm_bindgen::{JsCast, closure::Closure};

pub const TOKEN_KEY: &str = "auth_token";

// Token 信号由 App 根组件创建并通过 Context 共享
// 不能在子组件 owner 中创建，否则路由切换卸载组件时会连带 dispose 信号
pub fn auth_token() -> RwSignal<Option<String>> {
    use_context::<RwSignal<Option<String>>>().expect("auth_token 必须在 <App> 组件内使用")
}

#[component]
pub fn AuthExpiryWatcher() -> impl IntoView {
    let token = auth_token();
    let navigate = use_navigate();
    let window = web_sys::window().expect("浏览器窗口不可用");
    let callback = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        LocalStorage::delete(TOKEN_KEY);
        token.set(None);
        navigate("/login", Default::default());
    });
    let callback_ref = callback.as_ref().unchecked_ref();
    window
        .add_event_listener_with_callback("auth-expired", callback_ref)
        .expect("无法注册登录过期监听器");
    callback.forget();

    view! {}
}

#[component]
pub fn RequireAuth(children: ChildrenFn) -> impl IntoView {
    let token = auth_token();
    move || {
        if token.get().is_some() {
            children().into_any()
        } else {
            view! { <Redirect path="/login"/> }.into_any()
        }
    }
}
