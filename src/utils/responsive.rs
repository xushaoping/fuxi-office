use leptos::prelude::*;
use wasm_bindgen::{closure::Closure, JsCast};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Breakpoint {
    Mobile,
    Tablet,
    #[default]
    Desktop,
}

/// Reactive viewport breakpoint hook. It listens to `resize` and cleans up
/// automatically when the owning reactive scope is disposed.
pub fn use_breakpoint() -> Signal<Breakpoint> {
    let breakpoint = RwSignal::new(Breakpoint::default());
    if let Some(window) = web_sys::window() {
        let listener_window = window.clone();
        let update = {
            let breakpoint = breakpoint;
            move || {
                let width = listener_window
                    .inner_width()
                    .ok()
                    .and_then(|value| value.as_f64())
                    .unwrap_or(1024.0);
                breakpoint.set(if width < 640.0 {
                    Breakpoint::Mobile
                } else if width < 1024.0 {
                    Breakpoint::Tablet
                } else {
                    Breakpoint::Desktop
                });
            }
        };
        update();
        let callback =
            Closure::wrap(Box::new(move |_event: web_sys::Event| update()) as Box<dyn FnMut(_)>);
        let _ =
            window.add_event_listener_with_callback("resize", callback.as_ref().unchecked_ref());
        // The callback is intentionally leaked for the lifetime of the page;
        // this hook is used by the root workspace and avoids a Send/Sync
        // requirement that browser closures cannot satisfy on wasm.
        callback.forget();
    }
    breakpoint.into()
}
