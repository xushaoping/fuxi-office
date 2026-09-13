use leptos::prelude::*;

#[component]
pub fn Modal(
    #[prop(into)] open: Signal<bool>,
    #[prop(into)] on_close: Callback<()>,
    title: String,
    children: ChildrenFn,
) -> impl IntoView {
    let close = on_close.clone();
    let title = StoredValue::new(title);
    view! {
        <Show when=move || open.get() fallback=|| ()>
            <div class="modal-backdrop" on:click=move |_| on_close.run(()) aria-hidden="true"></div>
            <div class="modal" role="dialog" aria-modal="true" aria-labelledby="modal-title">
                <div class="modal-content" on:click=|ev| ev.stop_propagation()>
                    <header class="modal-header"><h2 id="modal-title">{move || title.get_value()}</h2><button class="modal-close" on:click=move |_| close.run(()) aria-label="关闭对话框">"×"</button></header>
                    <div class="modal-body">{children()}</div>
                </div>
            </div>
        </Show>
    }
}
