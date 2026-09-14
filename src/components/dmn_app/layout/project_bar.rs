use leptos::prelude::*;

use crate::components::dmn_app::context::use_app_context;
use crate::models::{DecisionProject, DecisionVersion};

#[component]
pub fn ProjectBar(
    projects: ReadSignal<Vec<DecisionProject>>,
    versions: ReadSignal<Vec<DecisionVersion>>,
    selected_version_id: ReadSignal<Option<i64>>,
    project_busy: ReadSignal<bool>,
    version_saving: ReadSignal<bool>,
    review_loading: ReadSignal<bool>,
    version_save_status: ReadSignal<Option<String>>,
    project_error: ReadSignal<Option<String>>,
    workspace_draft_storage_error: ReadSignal<Option<String>>,
    on_select_project: Callback<String>,
    on_select_version_review: Callback<String>,
) -> impl IntoView {
    let ctx = use_app_context();

    view! {
        <section
            class=move || if ctx.current_project.get().is_none() {
                "project-bar project-bar-empty"
            } else {
                "project-bar"
            }
            aria-label="决策项目"
        >
            <div class="project-current">
                <span class="toolbar-label">"当前项目"</span>
                <strong>
                    {move || ctx.current_project.get()
                        .map(|project| project.name)
                        .unwrap_or_else(|| "尚未选择项目".into())
                    }
                </strong>
                <small>
                    {move || ctx.current_version.get()
                        .map(|version| format!("v{} · {}", version.version_number, version.status))
                        .unwrap_or_else(|| "暂无版本".into())
                    }
                </small>
                <Show when=move || ctx.current_project.get().is_none()>
                    <span class="project-required-badge">"需要先选择"</span>
                </Show>
            </div>
            <div class="project-actions">
                <select
                    id="decision-project-selector"
                    class="project-select"
                    aria-label="切换决策项目"
                    prop:value=move || ctx.current_project.get()
                        .map(|project| project.id.to_string())
                        .unwrap_or_default()
                    on:change=move |event| {
                        let selected = event_target_value(&event);
                        event_target::<web_sys::HtmlSelectElement>(&event).set_value(
                            &ctx.current_project.get_untracked()
                                .map(|project| project.id.to_string())
                                .unwrap_or_default(),
                        );
                        on_select_project.run(selected);
                    }
                    disabled=move || project_busy.get() || version_saving.get() || projects.get().is_empty()
                >
                    <option value="">"选择项目"</option>
                    <For each=move || projects.get() key=|project| project.id let:project>
                        <option value=project.id.to_string()>{project.name}</option>
                    </For>
                </select>
                <select
                    class="project-select version-select"
                    aria-label="查看版本变更"
                    prop:value=move || selected_version_id.get()
                        .map(|id| id.to_string())
                        .unwrap_or_default()
                    on:change=move |event| on_select_version_review.run(event_target_value(&event))
                    disabled=move || review_loading.get() || versions.get().is_empty()
                >
                    <option value="">
                        {move || if review_loading.get() { "读取审查..." } else { "版本变更" }}
                    </option>
                    <For each=move || versions.get() key=|version| version.id let:version>
                        <option value=version.id.to_string()>
                            {format!("v{} · {}", version.version_number, version.status)}
                        </option>
                    </For>
                </select>
            </div>
            <div class="project-status">
                <span>
                    {move || version_save_status.get()
                        .unwrap_or_else(|| "生成结果需确认变更后保存为草稿版本".into())
                    }
                </span>
                <Show when=move || project_error.get().is_some()>
                    <strong>{move || project_error.get().unwrap_or_default()}</strong>
                </Show>
                <Show when=move || workspace_draft_storage_error.get().is_some()>
                    <strong class="project-storage-warning" role="alert">
                        {move || workspace_draft_storage_error.get().unwrap_or_default()}
                    </strong>
                </Show>
            </div>
        </section>
    }
}
