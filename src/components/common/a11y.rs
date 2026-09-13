use leptos::prelude::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LivePoliteness {
    #[default]
    Polite,
    Assertive,
    Off,
}

#[component]
pub fn LiveRegion(
    #[prop(into)] message: Signal<Option<String>>,
    #[prop(optional)] politeness: LivePoliteness,
) -> impl IntoView {
    let aria_live = match politeness {
        LivePoliteness::Polite => "polite",
        LivePoliteness::Assertive => "assertive",
        LivePoliteness::Off => "off",
    };
    view! { <div class="sr-only" aria-live=aria_live aria-atomic="true">{move || message.get()}</div> }
}
