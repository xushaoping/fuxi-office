use leptos::prelude::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoadingSize {
    Small,
    #[default]
    Medium,
    Large,
}

#[component]
pub fn LoadingSpinner(
    #[prop(optional)] size: LoadingSize,
    #[prop(optional)] message: Option<String>,
) -> impl IntoView {
    let size_class = match size {
        LoadingSize::Small => "loading-sm",
        LoadingSize::Medium => "loading-md",
        LoadingSize::Large => "loading-lg",
    };
    view! {
        <div class="loading-container" role="status" aria-live="polite">
            <span class=format!("loading-spinner {size_class}") aria-hidden="true">
                <svg viewBox="0 0 50 50"><circle cx="25" cy="25" r="20" fill="none" stroke="currentColor" stroke-width="4"/></svg>
            </span>
            {message.map(|text| view! { <span class="loading-message">{text}</span> })}
        </div>
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SkeletonVariant {
    #[default]
    Text,
    Circle,
    Rectangle,
}

#[component]
pub fn Skeleton(
    #[prop(optional)] width: Option<String>,
    #[prop(optional)] height: Option<String>,
    #[prop(optional)] variant: SkeletonVariant,
) -> impl IntoView {
    let class = match variant {
        SkeletonVariant::Text => "skeleton skeleton-text",
        SkeletonVariant::Circle => "skeleton skeleton-circle",
        SkeletonVariant::Rectangle => "skeleton skeleton-rect",
    };
    let style = format!(
        "width: {}; height: {};",
        width.unwrap_or_else(|| "100%".into()),
        height.unwrap_or_else(|| "20px".into())
    );
    view! { <div class=class style=style aria-hidden="true"></div> }
}
