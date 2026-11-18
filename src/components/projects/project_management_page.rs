use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;

use crate::api;
use crate::components::auth::auth_token;
use crate::components::common::{SelectOption, WorkbenchSelect};
use crate::models::{CreateDecisionProject, DecisionProject};
use crate::state::AppState;

fn risk_label(value: &str) -> &'static str {
    match value {
        "low" => "低风险",
        "high" => "高风险",
        _ => "中风险",
    }
}

#[component]
pub fn ProjectManagementPage() -> impl IntoView {
    let token = auth_token();
    let navigate = use_navigate();
    let app_state = expect_context::<AppState>();
    let projects = RwSignal::new(Vec::<DecisionProject>::new());
    let editing_id = RwSignal::new(None::<i64>);
    let name = RwSignal::new(String::new());
    let description = RwSignal::new(String::new());
    let risk_level = RwSignal::new("medium".to_string());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let status = RwSignal::new(None::<String>);

    let reload = move || {
        let Some(token_value) = token.get() else {
            return;
        };
        spawn_local(async move {
            match api::list_projects(&token_value).await {
                Ok(items) => projects.set(items),
                Err(message) => error.set(Some(message)),
            }
        });
    };
    Effect::new(move |_| {
        if token.get().is_some() {
            reload();
        }
    });

    let clear_form = move || {
        editing_id.set(None);
        name.set(String::new());
        description.set(String::new());
        risk_level.set("medium".into());
        error.set(None);
        status.set(None);
    };
    let edit_project = move |project: DecisionProject| {
        editing_id.set(Some(project.id));
        name.set(project.name);
        description.set(project.description);
        risk_level.set(project.risk_level);
        error.set(None);
        status.set(None);
    };
    let save = move |_| {
        let Some(token_value) = token.get() else {
            return;
        };
        let project_name = name.get().trim().to_string();
        if project_name.is_empty() || busy.get() {
            return;
        }
        busy.set(true);
        error.set(None);
        status.set(None);
        let payload = CreateDecisionProject {
            name: project_name,
            description: description.get().trim().to_string(),
            risk_level: risk_level.get(),
        };
        let project_id = editing_id.get();
        spawn_local(async move {
            let result = match project_id {
                Some(project_id) => api::update_project(&token_value, project_id, payload).await,
                None => api::create_project(&token_value, payload).await,
            };
            match result {
                Ok(project) => {
                    if project_id.is_some() {
                        projects.update(|items| {
                            if let Some(item) = items.iter_mut().find(|item| item.id == project.id)
                            {
                                *item = project;
                            }
                        });
                        status.set(Some("项目已更新".into()));
                    } else {
                        projects.update(|items| items.insert(0, project));
                        status.set(Some("项目已创建".into()));
                    }
                    editing_id.set(None);
                    name.set(String::new());
                    description.set(String::new());
                    risk_level.set("medium".into());
                }
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };
    let delete_project = move |project: DecisionProject| {
        let Some(token_value) = token.get() else {
            return;
        };
        if busy.get()
            || !web_sys::window()
                .and_then(|window| {
                    window
                        .confirm_with_message(&format!(
                            "确定删除项目“{}”吗？此操作无法撤销。",
                            project.name
                        ))
                        .ok()
                })
                .unwrap_or(false)
        {
            return;
        }
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match api::delete_project(&token_value, project.id).await {
                Ok(()) => {
                    projects.update(|items| items.retain(|item| item.id != project.id));
                    if editing_id.get() == Some(project.id) {
                        clear_form();
                    }
                    status.set(Some("项目已删除".into()));
                }
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    view! {
        <main class="admin-page project-management-page">
            <header class="admin-topbar">
                <div class="admin-brand">
                    <div class="brand-mark"><img src="/images/fuxi.png" /></div>
                    <div>
                        <span class="eyebrow">"DMN DECISION WORKSPACE"</span>
                        <strong>"伏羲智能决策建模工作台"</strong>
                    </div>
                </div>
                <nav class="admin-topbar-actions admin-page-nav" aria-label="页面导航">
                    <button class="button button-quiet" on:click=move |_| {
                        if !app_state.ui.workspace_dirty.get_untracked()
                            || web_sys::window().is_some_and(|window| {
                                window.confirm_with_message("当前工作台有未确认的需求或模型变更，本地草稿会保留，确定返回吗？").unwrap_or(false)
                            })
                        {
                            navigate("/", Default::default());
                        }
                    }>
                        <span aria-hidden="true">"←"</span> "返回工作台"
                    </button>
                </nav>
            </header>
            <div class="admin-content">
                <section class="admin-heading">
                    <div>
                        <span class="yaml-kicker">"PROJECT DIRECTORY / GOVERNANCE"</span>
                        <h1>"项目管理"</h1>
                        <p>"集中维护项目基本信息，工作台仅负责项目切换和决策建模。"</p>
                    </div>
                    <div class="admin-summary">
                        <span class="admin-summary-label">"项目总数"</span>
                        <strong>{move || projects.get().len()}</strong>
                    </div>
                </section>
                <div class="admin-layout">
                    <section class="management-form-panel">
                        <div class="admin-section-heading">
                            <div><span class="yaml-kicker">"PROJECT FORM"</span><h2>{move || if editing_id.get().is_some() { "修改项目" } else { "添加项目" }}</h2></div>
                            <span class="admin-section-index">"01"</span>
                        </div>
                        <div class="admin-form-grid management-form-grid">
                            <label><span>"项目名称"</span><input maxlength="120" placeholder="输入项目名称" prop:value=name on:input=move |event| name.set(event_target_value(&event)) /></label>
                            <WorkbenchSelect
                                label="风险等级"
                                options=Signal::derive(|| vec![
                                    SelectOption::new("low", "低风险"),
                                    SelectOption::new("medium", "中风险"),
                                    SelectOption::new("high", "高风险"),
                                ])
                                value=risk_level
                                disabled=busy
                                placeholder=Signal::derive(|| "请选择风险等级".to_string())
                                on_change=Callback::new(move |value| risk_level.set(value))
                            />
                            <label class="management-form-wide"><span>"项目说明"</span><textarea maxlength="4000" rows="5" placeholder="补充项目用途、边界和责任说明" prop:value=description on:input=move |event| description.set(event_target_value(&event))></textarea></label>
                        </div>
                        <div class="project-management-form-actions">
                            <button class="button button-primary" on:click=save disabled=move || busy.get() || name.get().trim().is_empty()>{move || if busy.get() { "处理中..." } else if editing_id.get().is_some() { "保存修改" } else { "创建项目" }}</button>
                            <button class="button button-quiet" on:click=move |_| clear_form() disabled=move || busy.get()>"清空"</button>
                        </div>
                    </section>
                    <section class="management-list-panel">
                        <div class="admin-section-heading">
                            <div><span class="yaml-kicker">"PROJECT DIRECTORY"</span><h2>"项目列表"</h2></div>
                            <span class="admin-user-count">{move || projects.get().len()} " 个项目"</span>
                        </div>
                        <div class="project-management-list">
                            <For each=move || projects.get() key=|project| project.id let:project>
                                <article class="project-management-row">
                                    <div class="project-management-identity">
                                        <strong>{project.name.clone()}</strong>
                                        <small>{format!("负责人：{} · 更新于 {}", project.owner_username, project.updated_at.format("%Y-%m-%d %H:%M"))}</small>
                                        <p>{if project.description.is_empty() { "暂无项目说明".into() } else { project.description.clone() }}</p>
                                    </div>
                                    <div class="project-management-meta">
                                        <span class="admin-role-badge">{risk_label(&project.risk_level)}</span>
                                        <small>{if project.current_draft_version_id.is_some() { "已有草稿" } else { "尚无草稿" }}</small>
                                    </div>
                                    <div class="admin-user-actions">
                                        <button class="button button-quiet button-small" on:click={let project = project.clone(); move |_| edit_project(project.clone())} disabled=move || busy.get()>"编辑"</button>
                                        <button class="button button-danger button-small" on:click={let project = project.clone(); move |_| delete_project(project.clone())} disabled=move || busy.get()>"删除"</button>
                                    </div>
                                </article>
                            </For>
                            <Show when=move || projects.get().is_empty()><div class="admin-empty">"暂无项目，请先添加项目"</div></Show>
                        </div>
                    </section>
                </div>
                <Show when=move || status.get().is_some()><div class="member-status">{move || status.get().unwrap_or_default()}</div></Show>
                <Show when=move || error.get().is_some()><div class="error-msg">{move || error.get().unwrap_or_default()}</div></Show>
            </div>
        </main>
    }
}
