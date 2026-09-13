use leptos::{html, prelude::*};
use wasm_bindgen::JsCast;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    #[default]
    Primary,
    Secondary,
    Danger,
    Ghost,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonSize {
    Small,
    #[default]
    Medium,
    Large,
}

#[component]
pub fn Button(
    #[prop(optional)] variant: ButtonVariant,
    #[prop(optional)] size: ButtonSize,
    #[prop(optional)] disabled: bool,
    #[prop(optional)] loading: bool,
    #[prop(optional)] icon: Option<&'static str>,
    #[prop(optional)] on_click: Option<Callback<()>>,
    #[prop(optional)] class: &'static str,
    children: ChildrenFn,
) -> impl IntoView {
    let variant_class = match variant {
        ButtonVariant::Primary => "btn-primary",
        ButtonVariant::Secondary => "btn-secondary",
        ButtonVariant::Danger => "btn-danger",
        ButtonVariant::Ghost => "btn-ghost",
    };
    let size_class = match size {
        ButtonSize::Small => "btn-sm",
        ButtonSize::Medium => "btn-md",
        ButtonSize::Large => "btn-lg",
    };
    let is_disabled = disabled || loading;

    let button_ref = NodeRef::<html::Button>::new();

    let handle_click = move |ev: web_sys::MouseEvent| {
        // Create ripple effect
        if let Some(button) = button_ref.get() {
            let x = ev.offset_x() as f64;
            let y = ev.offset_y() as f64;

            if let Some(window) = web_sys::window() {
                if let Some(document) = window.document() {
                    if let Ok(ripple) = document.create_element("span") {
                        let _ = ripple.set_attribute("class", "ripple");
                        let _ =
                            ripple.set_attribute("style", &format!("left: {}px; top: {}px;", x, y));

                        let _ = button.append_child(&ripple);

                        // Remove ripple after animation
                        let ripple_clone = ripple.clone();
                        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                            wasm_bindgen::closure::Closure::once_into_js(move || {
                                let _ = ripple_clone.remove();
                            })
                            .unchecked_ref(),
                            600,
                        );
                    }
                }
            }
        }

        if !is_disabled {
            if let Some(callback) = on_click.as_ref() {
                callback.run(());
            }
        }
    };

    view! {
        <button
            node_ref=button_ref
            class=format!("btn {variant_class} {size_class} {class}")
            disabled=is_disabled
            on:click=handle_click
        >
            {loading.then(|| view! { <span class="btn-spinner" aria-label="处理中"></span> })}
            {icon.map(|value| view! { <span class="btn-icon" aria-hidden="true">{value}</span> })}
            <span class="btn-text">{children()}</span>
        </button>
    }
}
