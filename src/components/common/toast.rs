use crate::state::{AppState, ToastLevel, ToastMessage};
use leptos::prelude::*;
use wasm_bindgen::{JsCast, closure::Closure};

#[component]
pub fn ToastContainer() -> impl IntoView {
    let state = expect_context::<AppState>();
    view! {
        <div class="toast-container" aria-live="polite" aria-atomic="false">
            <For each=move || state.toast.messages.get() key=|toast| toast.id.clone() children=move |toast| view! { <Toast message=toast /> } />
        </div>
    }
}

#[component]
fn Toast(message: ToastMessage) -> impl IntoView {
    let state = expect_context::<AppState>();
    let visible = RwSignal::new(true);
    let id = message.id.clone();
    let dismiss_state = state.clone();
    let dismiss_id = id.clone();
    let dismiss = move || {
        visible.set(false);
        dismiss_state.dismiss_toast(&dismiss_id);
    };
    if let Some(duration) = message.duration {
        let visible = visible;
        let state = state.clone();
        let id = message.id.clone();
        let callback = Closure::once_into_js(move || {
            visible.set(false);
            state.dismiss_toast(&id);
        });
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                callback.unchecked_ref(),
                duration as i32,
            );
        }
    }
    let (class, icon) = match message.level {
        ToastLevel::Success => ("toast-success", "✓"),
        ToastLevel::Info => ("toast-info", "ℹ"),
        ToastLevel::Warning => ("toast-warning", "⚠"),
        ToastLevel::Error => ("toast-error", "✕"),
    };
    view! {
        <div class=move || format!("toast {class} {}", if visible.get() { "toast-visible" } else { "toast-hidden" }) role="alert">
            <span class="toast-icon" aria-hidden="true">{icon}</span>
            <div class="toast-content"><strong class="toast-title">{message.title}</strong><span class="toast-message">{message.message}</span></div>
            <button class="toast-close" on:click=move |_| dismiss() aria-label="关闭通知">"×"</button>
        </div>
    }
}
