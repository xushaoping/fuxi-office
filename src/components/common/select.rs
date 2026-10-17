use leptos::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

impl SelectOption {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }
}

#[component]
pub fn WorkbenchSelect(
    #[prop(into)] label: String,
    #[prop(into)] options: Signal<Vec<SelectOption>>,
    #[prop(into)] value: Signal<String>,
    #[prop(into)] disabled: Signal<bool>,
    #[prop(into)] on_change: Callback<String>,
    #[prop(into)] placeholder: Signal<String>,
    #[prop(optional, into)] class: Option<String>,
    #[prop(optional, into)] id: Option<String>,
) -> impl IntoView {
    let (is_open, set_is_open) = signal(false);
    let trigger_id = id.clone();
    let aria_label = label.clone();

    view! {
        <div class=move || {
            let mut classes = String::from("wb-select-field");
            if let Some(class) = &class {
                classes.push(' ');
                classes.push_str(class);
            }
            classes
        }>
            <span class="wb-select-label">{label}</span>
            <div class=move || if is_open.get() { "wb-select is-open" } else { "wb-select" }>
                <button
                    type="button"
                    id=trigger_id
                    class="wb-select-trigger"
                    aria-label=aria_label.clone()
                    aria-haspopup="listbox"
                    aria-expanded=move || is_open.get().to_string()
                    disabled=move || disabled.get()
                    on:click=move |_| set_is_open.update(|open| *open = !*open)
                    on:keydown=move |event: leptos::ev::KeyboardEvent| {
                        if event.key() == "Escape" {
                            set_is_open.set(false);
                        }
                    }
                >
                    <span class=move || {
                        if value.get().is_empty() { "wb-select-value is-placeholder" } else { "wb-select-value" }
                    }>
                        {move || {
                            let current_value = value.get();
                            options.get().into_iter()
                                .find(|option| option.value == current_value)
                                .map(|option| option.label)
                                .unwrap_or_else(|| placeholder.get())
                        }}
                    </span>
                    <span class="wb-select-chevron" aria-hidden="true"></span>
                </button>
                <div
                    class=move || if is_open.get() { "wb-select-menu is-open" } else { "wb-select-menu" }
                    role="listbox"
                    aria-label=aria_label
                >
                    {move || options.get().into_iter().map(|option| {
                        let option_value = option.value.clone();
                        let option_label = option.label;
                        let class_value = option_value.clone();
                        let aria_value = option_value.clone();
                        let selected_value = option_value.clone();
                        view! {
                            <button
                                type="button"
                                class=move || if value.get() == class_value { "wb-select-option is-selected" } else { "wb-select-option" }
                                role="option"
                                aria-selected=move || (value.get() == aria_value).to_string()
                                on:click=move |_| {
                                    on_change.run(option_value.clone());
                                    set_is_open.set(false);
                                }
                            >
                                <span>{option_label}</span>
                                <Show when=move || value.get() == selected_value>
                                    <span class="wb-select-check" aria-hidden="true">"✓"</span>
                                </Show>
                            </button>
                        }
                    }).collect_view()}
                    <Show when=move || options.get().is_empty()>
                        <div class="wb-select-empty">{placeholder.get()}</div>
                    </Show>
                </div>
            </div>
        </div>
    }
}
