use leptos::prelude::*;

#[component]
pub fn PageTransition(children: ChildrenFn) -> impl IntoView {
    view! { <div class="page-transition">{children()}</div> }
}
