use leptos::prelude::*;

use crate::components::dmn_app::context::use_app_context;
use crate::components::dmn_app::types::{ResourceView, RunStatus, WorkspaceView};
use crate::models::{DecisionProject, DecisionVersion};

#[component]
pub fn ProjectBar(
    projects: ReadSignal<Vec<DecisionProject>>,
    versions: ReadSignal<Vec<DecisionVersion>>,
    selected_version_id: ReadSignal<Option<i64>>,
    new_project_name: ReadSignal<String>,
    set_new_project_name: WriteSignal<String>,
    project_busy: ReadSignal<bool>,
    version_saving: ReadSignal<bool>,
    review_loading: ReadSignal<bool>,
    version_save_status: ReadSignal<Option<String>>,
    project_error: ReadSignal<Option<String>>,
    workspace_view: ReadSignal<WorkspaceView>,
    resource_view: ReadSignal<ResourceView>,
    status: ReadSignal<RunStatus>,
    on_select_project: Callback<String>,
    on_select_version_review: Callback<String>,
    on_create_project: Callback<()>,
    on_delete_project: Callback<()>,
    on_toggle_model_library: Callback<()>,
    on_toggle_knowledge_library: Callback<()>,
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
                    on:change=move |event| on_select_project.run(event_target_value(&event))
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
                <button
                    class=move || if workspace_view.get() == WorkspaceView::Resources
                        && resource_view.get() == ResourceView::Models {
                        "button button-quiet is-active"
                    } else {
                        "button button-quiet"
                    }
                    on:click=move |_| on_toggle_model_library.run(())
                    disabled=move || project_busy.get() || version_saving.get()
                >"模型库"</button>
                <button
                    class=move || if workspace_view.get() == WorkspaceView::Resources
                        && resource_view.get() == ResourceView::Knowledge {
                        "button button-quiet is-active"
                    } else {
                        "button button-quiet"
                    }
                    on:click=move |_| on_toggle_knowledge_library.run(())
                    disabled=move || project_busy.get() || version_saving.get()
                >"知识库"</button>
                <input
                    class="project-name-input"
                    type="text"
                    maxlength="120"
                    placeholder="新项目名称"
                    prop:value=new_project_name
                    on:input=move |event| set_new_project_name.set(event_target_value(&event))
                    disabled=move || project_busy.get() || version_saving.get()
                />
                <button
                    class="button button-quiet"
                    on:click=move |_| on_create_project.run(())
                    disabled=move || project_busy.get()
                        || version_saving.get()
                        || new_project_name.get().trim().is_empty()
                >
                    {move || if project_busy.get() { "处理中..." } else { "创建项目" }}
                </button>
                <button
                    class="button button-danger"
                    title="永久删除当前项目及其全部数据"
                    on:click=move |_| on_delete_project.run(())
                    disabled=move || ctx.current_project.get().is_none()
                        || project_busy.get()
                        || version_saving.get()
                        || status.get() == RunStatus::Running
                >"删除当前项目"</button>
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
            </div>
        </section>
    }
}
