use chrono::{Datelike, Months, NaiveDate};
use leptos::leptos_dom::helpers::window_event_listener;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

fn local_today() -> NaiveDate {
    let now = js_sys::Date::new_0();
    NaiveDate::from_ymd_opt(
        now.get_full_year() as i32,
        now.get_month() + 1,
        now.get_date(),
    )
    .expect("browser date must be valid")
}

#[component]
pub fn WorkbenchDatePicker(
    #[prop(into)] label: String,
    #[prop(into)] value: Signal<String>,
    #[prop(into)] disabled: Signal<bool>,
    #[prop(into)] on_change: Callback<String>,
) -> impl IntoView {
    let (is_open, set_is_open) = signal(false);
    let (cursor, set_cursor) = signal(local_today().with_day(1).unwrap());
    let wrapper = NodeRef::<leptos::html::Div>::new();
    let trigger = NodeRef::<leptos::html::Button>::new();
    let aria_label = label.clone();
    let dialog_label = format!("{label}，选择日期");
    let selected = Signal::derive(move || NaiveDate::parse_from_str(&value.get(), "%Y-%m-%d").ok());
    let close = move || {
        set_is_open.set(false);
        if let Some(element) = trigger.get() {
            let _ = element.focus();
        }
    };
    let choose = move |date: Option<NaiveDate>| {
        if disabled.get_untracked() {
            return;
        }
        on_change.run(
            date.map(|date| date.format("%Y-%m-%d").to_string())
                .unwrap_or_default(),
        );
        close();
    };
    let outside_click = window_event_listener(leptos::ev::click, move |event| {
        if let Some(element) = wrapper.get()
            && !element.contains(
                event
                    .target()
                    .as_ref()
                    .and_then(|target| target.dyn_ref::<web_sys::Node>()),
            )
        {
            set_is_open.set(false);
        }
    });
    on_cleanup(move || outside_click.remove());
    Effect::new(move |_| {
        if disabled.get() {
            set_is_open.set(false);
        }
    });

    view! {
        <div
            class="wb-date-field"
            node_ref=wrapper
            on:keydown=move |event: leptos::ev::KeyboardEvent| {
                if event.key() == "Escape" && is_open.get() {
                    event.prevent_default();
                    event.stop_propagation();
                    close();
                }
            }
            on:focusout=move |event: leptos::ev::FocusEvent| {
                if let Some(element) = wrapper.get()
                    && !element.contains(event.related_target().as_ref().and_then(|target| target.dyn_ref::<web_sys::Node>()))
                {
                    set_is_open.set(false);
                }
            }
        >
            <span class="wb-select-label">{label}</span>
            <div class="wb-date-picker">
                <button
                    type="button"
                    node_ref=trigger
                    class="wb-select-trigger wb-date-trigger"
                    aria-label=aria_label
                    aria-haspopup="dialog"
                    aria-expanded=move || is_open.get().to_string()
                    disabled=move || disabled.get()
                    on:click=move |_| {
                        if !is_open.get() {
                            set_cursor.set(selected.get().unwrap_or_else(local_today).with_day(1).unwrap());
                        }
                        set_is_open.update(|open| *open = !*open);
                    }
                >
                    <span class=move || if selected.get().is_some() { "wb-select-value" } else { "wb-select-value is-placeholder" }>
                        {move || selected.get().map(|date| date.format("%Y-%m-%d").to_string()).unwrap_or_else(|| "请选择日期".into())}
                    </span>
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true">
                        <rect x="3" y="4" width="18" height="18" rx="2"></rect>
                        <path d="M16 2v4M8 2v4M3 10h18"></path>
                    </svg>
                </button>
                <Show when=move || is_open.get() && !disabled.get()>
                    <div class="wb-date-panel" role="dialog" aria-label=dialog_label.clone()>
                        <div class="wb-calendar-header">
                            <button type="button" aria-label="上个月" on:click=move |_| {
                                if let Some(month) = cursor.get().checked_sub_months(Months::new(1)) {
                                    set_cursor.set(month);
                                }
                            }>"‹"</button>
                            <strong aria-live="polite">{move || cursor.get().format("%Y 年 %m 月").to_string()}</strong>
                            <button type="button" aria-label="下个月" on:click=move |_| {
                                if let Some(month) = cursor.get().checked_add_months(Months::new(1)) {
                                    set_cursor.set(month);
                                }
                            }>"›"</button>
                        </div>
                        <div class="wb-calendar-weekdays" aria-hidden="true">
                            {["日", "一", "二", "三", "四", "五", "六"].into_iter().map(|day| view! { <span>{day}</span> }).collect_view()}
                        </div>
                        <div class="wb-calendar-days">
                            {move || {
                                let month = cursor.get();
                                let today = local_today();
                                let mut cells = Vec::new();
                                for _ in 0..month.weekday().num_days_from_sunday() {
                                    cells.push(view! { <span aria-hidden="true"></span> }.into_any());
                                }
                                for day in 1..=31 {
                                    let Some(date) = month.with_day(day) else { break };
                                    cells.push(view! {
                                        <button
                                            type="button"
                                            class=move || if selected.get() == Some(date) { "wb-calendar-day is-selected" } else { "wb-calendar-day" }
                                            aria-label=date.format("%Y-%m-%d").to_string()
                                            aria-pressed=move || selected.get() == Some(date)
                                            aria-current=if date == today { Some("date") } else { None }
                                            on:click=move |_| choose(Some(date))
                                        >{day}</button>
                                    }.into_any());
                                }
                                cells.collect_view()
                            }}
                        </div>
                        <div class="wb-calendar-footer">
                            <button type="button" on:click=move |_| choose(None)>"清空"</button>
                            <button type="button" on:click=move |_| choose(Some(local_today()))>"今天"</button>
                        </div>
                    </div>
                </Show>
            </div>
        </div>
    }
}
