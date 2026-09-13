use leptos::prelude::*;

use crate::models::{AuditEvent, ProjectMember};

/// 审计事件类型分类
pub fn audit_event_kind(event_type: &str) -> &'static str {
    match event_type {
        "test_cases.generated" | "test_case.approved" | "test_case.approval_revoked" => "test-case",
        "version.verified" => "verification",
        "version.published" => "published",
        "version.retired" => "retired",
        _ => "general",
    }
}

/// 审计事件类型标签
pub fn audit_event_type_label(event_type: &str) -> &'static str {
    match event_type {
        "project.created" => "项目创建",
        "model.reused" => "模型复用",
        "version.created" => "版本创建",
        "version.rollback_created" => "回滚草稿",
        "version.verified" => "自动验收",
        "version.reviewed" => "独立复核",
        "version.published" => "版本发布",
        "version.retired" => "版本退役",
        "test_cases.generated" => "案例生成",
        "test_case.approved" => "案例批准",
        "test_case.approval_revoked" => "撤销批准",
        "decision_service.invoked" => "服务调用",
        "project.member_added" => "成员添加",
        "project.member_removed" => "成员移除",
        _ => "其他事件",
    }
}

#[component]
pub fn MemberManagementPanel(
    members: ReadSignal<Vec<ProjectMember>>,
    member_username: ReadSignal<String>,
    set_member_username: WriteSignal<String>,
    member_role: ReadSignal<String>,
    set_member_role: WriteSignal<String>,
    member_busy: ReadSignal<bool>,
    member_error: ReadSignal<Option<String>>,
    member_status: ReadSignal<Option<String>>,
    on_save_member: Callback<()>,
    on_remove_member: Callback<(String, String)>,
) -> impl IntoView {
    view! {
        <section class="member-management-panel" aria-label="成员管理">
            <div class="panel-heading">
                <strong>"团队成员"</strong>
                <small>{move || format!("{} 位成员", members.get().len())}</small>
            </div>
            <div class="member-list">
                <For each=move || members.get() key=|member| format!("{}:{}", member.username, member.role) let:member>
                    <div class="member-item">
                        <div>
                            <strong>{member.username.clone()}</strong>
                            <span>{member.role.clone()}</span>
                        </div>
                        <button
                            class="button button-quiet button-danger"
                            on:click=move |_| {
                                on_remove_member.run((member.username.clone(), member.role.clone()))
                            }
                            disabled=member_busy
                        >"移除"</button>
                    </div>
                </For>
            </div>
            <div class="member-form">
                <input
                    type="text"
                    placeholder="用户名"
                    prop:value=member_username
                    on:input=move |event| set_member_username.set(event_target_value(&event))
                    disabled=member_busy
                />
                <select
                    prop:value=member_role
                    on:change=move |event| set_member_role.set(event_target_value(&event))
                    disabled=member_busy
                >
                    <option value="modeler">"建模者"</option>
                    <option value="reviewer">"复核者"</option>
                    <option value="approver">"审批者"</option>
                    <option value="viewer">"查看者"</option>
                </select>
                <button
                    class="button button-primary"
                    on:click=move |_| on_save_member.run(())
                    disabled=move || member_busy.get() || member_username.get().trim().is_empty()
                >
                    {move || if member_busy.get() { "处理中..." } else { "添加成员" }}
                </button>
            </div>
            <Show when=move || member_error.get().is_some()>
                <div class="member-error">{move || member_error.get().unwrap_or_default()}</div>
            </Show>
            <Show when=move || member_status.get().is_some()>
                <div class="member-status">{move || member_status.get().unwrap_or_default()}</div>
            </Show>
        </section>
    }
}

#[component]
pub fn AuditLogPanel(events: ReadSignal<Vec<AuditEvent>>) -> impl IntoView {
    view! {
        <div class="audit-timeline" aria-label="项目审计记录">
            <Show
                when=move || !events.get().is_empty()
                fallback=move || view! { <div class="audit-empty">"暂无审计事件"</div> }
            >
                <For each=move || events.get() key=|event| event.id let:event>
                    <article class=format!("audit-event audit-type-{}", audit_event_kind(&event.event_type))>
                        <span class="audit-event-dot" aria-hidden="true"></span>
                        <div>
                            <strong>{event.summary}</strong>
                            <small>{format!("{} · {}", event.actor_username, event.created_at.format("%m-%d %H:%M:%S"))}</small>
                            <code>{audit_event_type_label(&event.event_type)}</code>
                        </div>
                    </article>
                </For>
            </Show>
        </div>
    }
}

#[component]
pub fn RollbackPanel(
    rollback_reason: ReadSignal<String>,
    set_rollback_reason: WriteSignal<String>,
    rollback_confirmed: ReadSignal<bool>,
    set_rollback_confirmed: WriteSignal<bool>,
    rollback_busy: ReadSignal<bool>,
    on_create_rollback: Callback<()>,
) -> impl IntoView {
    view! {
        <div class="rollback-controls">
            <div class="rollback-copy">
                <strong>"创建回滚草稿"</strong>
                <small>"复制所选历史版本，线上版本保持不变；新草稿必须重新验收和发布。"</small>
            </div>
                    <textarea
                        rows="2"
                        maxlength="500"
                        placeholder="填写回滚原因"
                        prop:value=rollback_reason
                        on:input=move |event| {
                            set_rollback_reason.set(event_target_value(&event));
                            set_rollback_confirmed.set(false);
                        }
                        disabled=rollback_busy
                    ></textarea>
                    <label class="rollback-confirmation">
                        <input
                            type="checkbox"
                            prop:checked=rollback_confirmed
                            on:change=move |event| set_rollback_confirmed.set(event_target_checked(&event))
                            disabled=rollback_busy
                        />
                        <span>"我已确认回滚差异，并接受重新验收后才能发布"</span>
                    </label>
                    <button
                        class="button button-primary"
                        on:click=move |_| on_create_rollback.run(())
                        disabled=move || rollback_busy.get()
                            || rollback_reason.get().trim().is_empty()
                            || !rollback_confirmed.get()
                    >
                        {move || if rollback_busy.get() { "创建中..." } else { "创建回滚草稿" }}
                    </button>
        </div>
    }
}
