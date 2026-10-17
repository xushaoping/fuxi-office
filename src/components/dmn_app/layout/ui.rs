use leptos::prelude::*;

#[component]
pub(super) fn WorkbenchIcon(name: &'static str) -> impl IntoView {
    let path = match name {
        "discovery" => {
            "M12 3 9.5 8.5 4 11l5.5 2.5L12 19l2.5-5.5L20 11l-5.5-2.5L12 3ZM4 3v4M2 5h4M20 16v6M17 19h6"
        }
        "model" => "m12 3 9 5-9 5-9-5 9-5ZM3 12l9 5 9-5M3 16l9 5 9-5",
        "validation" => "m9 12 2 2 4-4M12 3l8 3v6c0 5-8 9-8 9s-8-4-8-9V6l8-3Z",
        "services" => {
            "M8 17H6a4 4 0 0 1-1-7.9A7 7 0 0 1 18.5 8 4.5 4.5 0 0 1-.5 9h-2M12 12v9m-3-3 3 3 3-3"
        }
        "analytics" => "M4 3v17h17M8 15v-4m5 4V7m5 8v-6",
        "versions" => "M3 11a9 9 0 1 1 2.6 7.4M3 4v7h7m2-4v5l3 2",
        "collaboration" => {
            "M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2m20 0v-2a4 4 0 0 0-3-3.9M9 3a4 4 0 1 1 0 8 4 4 0 0 1 0-8Zm8 0a4 4 0 0 1 0 8"
        }
        "resources" => {
            "M12 6v15M3 3h3a6 6 0 0 1 6 3 6 6 0 0 1 6-3h3v15h-3a6 6 0 0 0-6 3 6 6 0 0 0-6-3H3V3Z"
        }
        "folder" => {
            "M3 7V5a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z"
        }
        "project" => {
            ""
        }
        "check" => "m5 12 4 4L19 6",
        "arrow" => "m9 5 7 7-7 7",
        "lock" => "M7 10V7a5 5 0 0 1 10 0v3M5 10h14v11H5V10Zm7 4v3",
        "logout" => "M9 3H4v18h5m6-14 5 5-5 5M9 12h11",
        _ => "M12 3v18M3 12h18",
    };

    view! {
        <svg class="wb-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d=path></path>
        </svg>
    }
}

#[component]
pub(super) fn WorkbenchTag(
    #[prop(default = "default")] tone: &'static str,
    children: Children,
) -> impl IntoView {
    view! { <span class=format!("wb-tag wb-tag-{tone}")>{children()}</span> }
}

#[component]
pub(super) fn WorkbenchStatistic(
    label: &'static str,
    value: Signal<String>,
    icon: &'static str,
) -> impl IntoView {
    view! {
        <div class="wb-statistic">
            <span class="wb-statistic-icon"><WorkbenchIcon name=icon /></span>
            <div>
                <span class="wb-statistic-label">{label}</span>
                <strong class="wb-statistic-value">{move || value.get()}</strong>
            </div>
        </div>
    }
}
