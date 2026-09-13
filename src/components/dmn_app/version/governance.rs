use leptos::prelude::*;

use crate::components::dmn_app::team::AuditLogPanel;
use crate::models::{AuditEvent, DecisionVersion};

/// 只负责版本时间线与审计展示，数据加载和页面切换仍由 DmnApp 管理。
#[component]
pub fn VersionGovernancePanel(
    versions: ReadSignal<Vec<DecisionVersion>>,
    current_version: ReadSignal<Option<DecisionVersion>>,
    audit_events: ReadSignal<Vec<AuditEvent>>,
) -> impl IntoView {
    view! {
        <section class="version-governance-panel" aria-label="版本时间线与审计">
            <div class="version-governance-heading">
                <div>
                    <span class="yaml-kicker">"VERSION GOVERNANCE"</span>
                    <strong>"版本时间线与审计"</strong>
                    <small>{move || format!("{} 个版本 · {} 条审计事件", versions.get().len(), audit_events.get().len())}</small>
                </div>
                <span class="governance-policy">"历史不可变"</span>
            </div>
            <div class="version-governance-grid">
                <VersionTimeline versions=versions current_version=current_version />
                <AuditLogPanel events=audit_events />
            </div>
        </section>
    }
}

#[component]
fn VersionTimeline(
    versions: ReadSignal<Vec<DecisionVersion>>,
    current_version: ReadSignal<Option<DecisionVersion>>,
) -> impl IntoView {
    view! {
        <div class="version-chain" aria-label="版本链">
            <For each=move || versions.get() key=|version| version.id let:version>
                {let status = version.status.clone();
                let source_label = version.rollback_source_version_id.and_then(|source_id| {
                    versions.get().into_iter().find(|item| item.id == source_id)
                        .map(|item| format!("回滚自 v{}", item.version_number))
                });
                let rollback_reason = version.rollback_reason.clone();
                let is_current = current_version.get().is_some_and(|current| current.id == version.id);
                view! {
                    <article class=format!("version-node version-node-{status}{}", if is_current { " is-current" } else { "" })>
                        <span class="version-node-marker" aria-hidden="true"></span>
                        <div class="version-node-body">
                            <div class="version-node-title">
                                <strong>{format!("v{}", version.version_number)}</strong>
                                <span>{status.clone()}</span>
                                {is_current.then(|| view! { <em>"当前"</em> })}
                            </div>
                            <small>{format!("{} · {}", version.created_by, version.created_at.format("%m-%d %H:%M"))}</small>
                            {source_label.map(|label| view! { <code>{label}</code> })}
                            {rollback_reason.map(|reason| view! { <p>{reason}</p> })}
                        </div>
                    </article>
                }}
            </For>
        </div>
    }
}
