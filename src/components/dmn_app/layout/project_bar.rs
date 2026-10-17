use leptos::prelude::*;

use super::ui::{WorkbenchIcon, WorkbenchTag};
use crate::components::common::{SelectOption, WorkbenchSelect};
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
                <span class="wb-project-icon">
                    <svg class="icon" viewBox="0 0 1024 1024" version="1.1" xmlns="http://www.w3.org/2000/svg" p-id="4241" width="200" height="200">
                        <path d="M601.6 1024L193.2 780.6l7-474.7L615.6 74.5 1024 317.9l-7 474.7L601.6 1024z" fill="#C7EAF8" p-id="4242"></path>
                        <path d="M387.9 507.8L65.6 333.6c-6.5-3.6-13.8-5.1-20.3-5.1-23.2 0-45 20.3-45 48.6v340.5c0 35.6 18.1 67.5 47.9 83.5l322.3 174.2c6.5 3.6 13.8 5.8 20.3 5.8 23.2 0 45-21 45-49.4V591.3c0-34.8-18.1-67.5-47.9-83.5z m-24.6 381.1l-281-151.7c-4.4-2.9-9.4-9.4-9.4-19.6V420l280.9 151.7c5.8 3.6 9.4 11.6 9.4 19.6v297.6zM899 328.5c-6.5 0-13.8 1.5-20.3 5.1l-322.4 175c-29.8 16-47.9 48.6-47.9 84.2v339c0 28.3 21.8 49.4 45 49.4 6.5 0 13.8-1.5 20.3-5.1L896 801.2c29-16 47.9-47.9 47.9-83.5V377.2c0.1-28.3-21.7-48.7-44.9-48.7z m-27.6 388.4c0 10.2-5.1 17.4-9.4 19.6L581 888.9V592.8c0-10.2 5.1-17.4 9.4-19.6l281-153.2v296.9zM848.9 253.8c15.2-8 22.5-21.8 21.1-34.8 1.5-13.8-5.8-26.9-21.1-34.8L520.8 12.8c-14.5-8-31.2-11.6-48.6-11.6-17.4 0-34.1 3.6-49.4 11.6L93.9 183.4c-15.2 8-21.8 21.8-21.1 35.6-0.7 13.1 5.8 26.9 21.1 34.8L422 424.4c14.5 8 31.9 11.6 49.4 11.6h1.5c17.4 0 34.1-3.6 49.4-11.6l326.6-170.6zM471.4 363.4c-5.8 0-11.6-1.5-16-3.6L184 218.9 455.4 77.4c4.4-2.2 10.2-3.6 16.7-3.6 5.8 0 11.6 1.5 16 3.6L759.6 219 487.4 359.8c-3.6 2.1-9.5 3.6-16 3.6z" fill="#8B8AFA" p-id="4243"></path>
                    </svg>
                </span>
                <div class="wb-project-copy">
                <span class="toolbar-label">"当前项目"</span>
                <strong>
                    {move || ctx.current_project.get()
                        .map(|project| project.name)
                        .unwrap_or_else(|| "尚未选择项目".into())
                    }
                </strong>
                <WorkbenchTag tone="primary">
                    {move || ctx.current_version.get()
                        .map(|version| format!("v{} · {}", version.version_number, match version.status.as_str() {
                            "draft" => "草稿",
                            "verified" => "已验收",
                            "published" => "已发布",
                            "retired" => "已退役",
                            _ => version.status.as_str(),
                        }))
                        .unwrap_or_else(|| "暂无版本".into())
                    }
                </WorkbenchTag>
                <Show when=move || ctx.current_project.get().is_none()>
                    <span class="project-required-badge">"需要先选择"</span>
                </Show>
                </div>
            </div>
            <div class="project-actions">
                <WorkbenchSelect
                    id="decision-project-selector"
                    class="project-select"
                    label="切换项目"
                    placeholder=Signal::derive(|| "选择项目".to_string())
                    options=Signal::derive(move || projects.get().into_iter().map(|project| {
                        SelectOption::new(project.id.to_string(), project.name)
                    }).collect())
                    value=Signal::derive(move || ctx.current_project.get()
                        .map(|project| project.id.to_string()).unwrap_or_default())
                    disabled=Signal::derive(move || project_busy.get() || version_saving.get() || projects.get().is_empty())
                    on_change=Callback::new(move |selected| on_select_project.run(selected))
                />
                <WorkbenchSelect
                    id="decision-version-selector"
                    class="project-select version-select"
                    label="版本变更"
                    placeholder=Signal::derive(move || if review_loading.get() { "读取审查...".to_string() } else { "版本变更".to_string() })
                    options=Signal::derive(move || versions.get().into_iter().map(|version| {
                        SelectOption::new(version.id.to_string(), format!("v{} · {}", version.version_number, version.status))
                    }).collect())
                    value=Signal::derive(move || selected_version_id.get().map(|id| id.to_string()).unwrap_or_default())
                    disabled=Signal::derive(move || review_loading.get() || versions.get().is_empty())
                    on_change=Callback::new(move |selected| on_select_version_review.run(selected))
                />
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
