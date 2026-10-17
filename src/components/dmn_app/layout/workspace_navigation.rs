use leptos::ev::KeyboardEvent;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use super::ui::WorkbenchIcon;
use crate::components::dmn_app::context::use_app_context;
use crate::components::workspace::WorkspaceView;

#[component]
pub fn WorkspaceNavigation(
    active: ReadSignal<WorkspaceView>,
    model_available: Signal<bool>,
    version_count: Signal<usize>,
    test_count: Signal<usize>,
    service_count: Signal<usize>,
    finding_count: Signal<usize>,
    member_count: Signal<usize>,
    on_select: Callback<WorkspaceView>,
) -> impl IntoView {
    let ctx = use_app_context();
    let is_enabled = move |view| match view {
        WorkspaceView::Discovery | WorkspaceView::Resources => true,
        WorkspaceView::Model => model_available.get(),
        WorkspaceView::Validation => ctx.current_version.get().is_some(),
        WorkspaceView::Services | WorkspaceView::Analytics => ctx
            .current_project
            .get()
            .is_some_and(|project| project.published_version_id.is_some()),
        WorkspaceView::Versions => version_count.get() > 0,
        WorkspaceView::Collaboration => ctx.current_project.get().is_some(),
    };
    let on_keydown = move |event: KeyboardEvent| {
        let key = event.key();
        if !matches!(key.as_str(), "ArrowLeft" | "ArrowRight" | "Home" | "End") {
            return;
        }
        let enabled: Vec<_> = WorkspaceView::ALL
            .into_iter()
            .filter(|view| is_enabled(*view))
            .collect();
        let Some(current) = enabled.iter().position(|view| *view == active.get()) else {
            return;
        };
        let target = match key.as_str() {
            "Home" => 0,
            "End" => enabled.len() - 1,
            "ArrowRight" => (current + 1) % enabled.len(),
            "ArrowLeft" => (current + enabled.len() - 1) % enabled.len(),
            _ => return,
        };
        event.prevent_default();
        let view = enabled[target];
        on_select.run(view);
        if let Some(element) =
            document().get_element_by_id(&format!("workspace-tab-{}", view.key()))
            && let Ok(element) = element.dyn_into::<web_sys::HtmlElement>()
        {
            let _ = element.focus();
        }
    };

    view! {
        <nav class="workspace-view-tabs wb-tabs" aria-label="项目工作区" role="tablist" aria-orientation="horizontal">
            {WorkspaceView::ALL.into_iter().map(|view| {
                let count = move || match view {
                    WorkspaceView::Validation => Some(test_count.get()),
                    WorkspaceView::Services => Some(service_count.get()),
                    WorkspaceView::Analytics => Some(finding_count.get()),
                    WorkspaceView::Versions => Some(version_count.get()),
                    WorkspaceView::Collaboration => Some(member_count.get()),
                    _ => None,
                };
                view! {
                    <button
                        type="button"
                        id=format!("workspace-tab-{}", view.key())
                        aria-controls=format!("workspace-panel-{}", view.key())
                        tabindex=move || if active.get() == view { "0" } else { "-1" }
                        class=move || if active.get() == view { "workspace-view-tab is-active" } else { "workspace-view-tab" }
                        role="tab"
                        aria-selected=move || active.get() == view
                        disabled=move || !is_enabled(view)
                        title=move || if is_enabled(view) { view.description() } else { view.unlock_hint() }
                        on:click=move |_| if is_enabled(view) { on_select.run(view); }
                        on:keydown=on_keydown
                    >
                        <WorkbenchIcon name=view.key() />
                        {view.label()}
                        {move || count().map(|value| view! { <span class="wb-tab-count">{value}</span> })}
                    </button>
                }
            }).collect_view()}
        </nav>
    }
}
