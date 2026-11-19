use leptos::prelude::*;

#[component]
pub fn WorkbenchNumberInput(
    #[prop(into)] label: String,
    value: RwSignal<String>,
    #[prop(default = 1)] min: i32,
    #[prop(default = 1)] step: i32,
    #[prop(optional)] max: Option<i32>,
    #[prop(into)] disabled: Signal<bool>,
    #[prop(optional, into)] class: Option<String>,
) -> impl IntoView {
    let decrement_value = value;
    let increment_value = value;
    let minimum = min.max(0);
    let maximum = max.map(|maximum| maximum.max(minimum));
    let step = step.max(1);
    let aria_label = label.clone();
    let increment_disabled = Signal::derive(move || {
        disabled.get()
            || maximum.is_some_and(|maximum| {
                increment_value.get().parse::<i32>().unwrap_or(minimum) >= maximum
            })
    });
    let decrement_disabled = Signal::derive(move || {
        disabled.get() || decrement_value.get().parse::<i32>().unwrap_or(minimum) <= minimum
    });

    let adjust = move |amount: i32| {
        if disabled.get_untracked() {
            return;
        }
        let current = value
            .get_untracked()
            .parse::<i32>()
            .unwrap_or(minimum)
            .max(minimum);
        let next = current.saturating_add(amount).max(minimum);
        let next = maximum.map_or(next, |maximum| next.min(maximum));
        value.set(next.to_string());
    };

    view! {
        <div class=move || {
            let mut classes = String::from("wb-number-field");
            if let Some(class) = &class {
                classes.push(' ');
                classes.push_str(class);
            }
            classes
        }>
            <label class="wb-number-label">{label}</label>
            <div
                class="wb-number-control"
                role="group"
                aria-label=aria_label
                aria-disabled=move || disabled.get().to_string()
            >
                <button
                    type="button"
                    class="wb-number-stepper"
                    aria-label="减少"
                    disabled=decrement_disabled
                    on:click=move |_| adjust(-step)
                >
                    <span aria-hidden="true">"−"</span>
                </button>
                <input
                    class="wb-number-input"
                    type="text"
                    inputmode="numeric"
                    autocomplete="off"
                    role="spinbutton"
                    aria-label=aria_label.clone()
                    aria-valuemin=minimum
                    aria-valuemax=maximum.map(|maximum| maximum.to_string())
                    aria-valuenow=move || value.get()
                    prop:value=move || value.get()
                    disabled=move || disabled.get()
                    on:input=move |event| {
                        let next = event_target_value(&event);
                        if next.is_empty() || next.bytes().all(|byte| byte.is_ascii_digit()) {
                            value.set(next);
                        }
                    }
                    on:blur=move |_| {
                        let next = value
                            .get_untracked()
                            .parse::<i32>()
                            .unwrap_or(minimum)
                            .max(minimum);
                        let next = maximum.map_or(next, |maximum| next.min(maximum));
                        value.set(next.to_string());
                    }
                    on:keydown=move |event: leptos::ev::KeyboardEvent| {
                        match event.key().as_str() {
                            "ArrowUp" => {
                                event.prevent_default();
                                adjust(step);
                            }
                            "ArrowDown" => {
                                event.prevent_default();
                                adjust(-step);
                            }
                            _ => {}
                        }
                    }
                />
                <button
                    type="button"
                    class="wb-number-stepper"
                    aria-label="增加"
                    disabled=increment_disabled
                    on:click=move |_| {
                        let current = increment_value
                            .get_untracked()
                            .parse::<i32>()
                            .unwrap_or(minimum)
                            .max(minimum);
                        let next = current.saturating_add(step);
                        increment_value.set(maximum.map_or(next, |maximum| next.min(maximum)).to_string());
                    }
                >
                    <span aria-hidden="true">"+"</span>
                </button>
            </div>
        </div>
    }
}
