use leptos::prelude::*;
use wasm_bindgen::{JsCast, closure::Closure};

/// Registers a global keyboard shortcut for the lifetime of the current view.
/// Inputs and textareas are ignored so normal text editing remains untouched.
pub fn use_keyboard_shortcut(key: &'static str, handler: impl Fn() + 'static) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let callback = Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
        let target = event
            .target()
            .and_then(|value| value.dyn_into::<web_sys::Element>().ok());
        let is_editing = target.as_ref().is_some_and(|element| {
            matches!(element.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT")
        });
        if !is_editing && event.key() == key {
            event.prevent_default();
            handler();
        }
    }) as Box<dyn FnMut(_)>);
    let _ = window.add_event_listener_with_callback("keydown", callback.as_ref().unchecked_ref());
    callback.forget();
}
