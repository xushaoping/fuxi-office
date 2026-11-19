use leptos::prelude::*;

#[component]
pub fn WorkbenchCheckbox(
    #[prop(into)] label: String,
    checked: Signal<bool>,
    disabled: Signal<bool>,
    on_change: Callback<bool>,
    #[prop(optional, into)] class: Option<String>,
    #[prop(default = false)] switch: bool,
) -> impl IntoView {
    view! {
        <label class=move || {
            let mut classes = String::from("wb-checkbox");
            if switch {
                classes.push_str(" is-switch");
            }
            if let Some(class) = &class {
                classes.push(' ');
                classes.push_str(class);
            }
            classes
        }>
            <input
                class="wb-checkbox-input"
                type="checkbox"
                prop:checked=move || checked.get()
                disabled=move || disabled.get()
                on:change=move |event| on_change.run(event_target_checked(&event))
            />
            <span class="wb-checkbox-indicator" aria-hidden="true"></span>
            <span class="wb-checkbox-label">{label}</span>
        </label>
    }
}
