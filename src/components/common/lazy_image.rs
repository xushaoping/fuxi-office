use leptos::prelude::*;

/// Native browser lazy-loading with an accessible placeholder state.
#[component]
pub fn LazyImage(
    src: String,
    alt: String,
    #[prop(optional)] placeholder: Option<String>,
) -> impl IntoView {
    let loaded = RwSignal::new(false);
    let fallback = placeholder.unwrap_or_default();
    let has_fallback = !fallback.is_empty();
    view! {
        <div class="lazy-image" class:lazy-image-loaded=move || loaded.get()>
            <img src=src alt=alt loading="lazy" decoding="async" on:load=move |_| loaded.set(true) />
            <Show when=move || !loaded.get() && has_fallback>
                <img class="lazy-image-placeholder" src=fallback.clone() alt="" aria-hidden="true" />
            </Show>
        </div>
    }
}
