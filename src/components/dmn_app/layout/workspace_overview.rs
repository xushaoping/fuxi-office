use leptos::prelude::*;

use super::ui::{WorkbenchIcon, WorkbenchStatistic, WorkbenchTag};
use crate::components::dmn_app::context::use_app_context;
use crate::components::workspace::WorkspaceView;

#[component]
pub fn WorkspaceOverview(
    active: ReadSignal<WorkspaceView>,
    test_count: Signal<usize>,
    version_count: Signal<usize>,
    service_count: Signal<usize>,
    has_unsaved_changes: Signal<bool>,
) -> impl IntoView {
    let ctx = use_app_context();

    view! {
        <section
            class=move || if active.get() == WorkspaceView::Model {
                "wb-overview is-model-overview"
            } else {
                "wb-overview"
            }
            aria-label="工作区概览"
        >
            <div class="wb-overview-heading">
                <div class="wb-overview-context">
                    <div class="wb-breadcrumb" aria-label="当前位置">
                        <span>"工作台"</span>
                        <WorkbenchIcon name="arrow" />
                        <span class="wb-overview-project" title=move || ctx.current_project.get().map(|project| project.name).unwrap_or_else(|| "未选择项目".into())>
                            {move || ctx.current_project.get().map(|project| project.name).unwrap_or_else(|| "未选择项目".into())}
                        </span>
                    </div>
                    <div class="wb-overview-status" role="status">
                        <Show when=move || has_unsaved_changes.get()>
                            <WorkbenchTag tone="warning">"有未确认变更"</WorkbenchTag>
                        </Show>
                        <Show when=move || !has_unsaved_changes.get() && ctx.current_version.get().is_some()>
                            <WorkbenchTag tone="primary">
                                {move || ctx.current_version.get().map(|version| format!("当前 v{}", version.version_number)).unwrap_or_default()}
                            </WorkbenchTag>
                        </Show>
                    </div>
                </div>
                <div class="wb-statistics" aria-label="项目数据">
                    <WorkbenchStatistic label="模型版本" value=Signal::derive(move || version_count.get().to_string()) icon="versions" />
                    <WorkbenchStatistic label="验收案例" value=Signal::derive(move || test_count.get().to_string()) icon="validation" />
                    <WorkbenchStatistic label="发布服务" value=Signal::derive(move || service_count.get().to_string()) icon="services" />
                </div>
            </div>
        </section>
    }
}
