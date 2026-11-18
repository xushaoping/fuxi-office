mod governance;
mod history;

pub use governance::VersionGovernancePanel;
pub use history::VersionHistoryPanel;

use leptos::prelude::*;

use crate::models::{VersionChangeReview, VersionDiffEntry, VersionVerificationReport};

#[component]
pub fn VersionChangeReviewView(
    review: VersionChangeReview,
    confirmed: ReadSignal<bool>,
    set_confirmed: WriteSignal<bool>,
    on_confirm: impl Fn(web_sys::MouseEvent) + Send + Sync + 'static,
    on_discard: impl Fn(web_sys::MouseEvent) + Send + Sync + 'static,
    saving: ReadSignal<bool>,
) -> impl IntoView {
    let on_confirm = Callback::new(on_confirm);
    let on_discard = Callback::new(on_discard);
    let changes = RwSignal::new(review.changes);
    let findings = RwSignal::new(review.findings);
    let risk_class = format!("review-risk risk-{}", review.overall_risk);
    view! {
        <section class="change-review-panel" aria-label="版本变更确认">
            <div class="change-review-heading">
                <div>
                    <span class="yaml-kicker">"VERSION GOVERNANCE"</span>
                    <strong>"变更确认"</strong>
                    <small>{review.summary}</small>
                </div>
                <span class=risk_class>{format!("{}风险", review.overall_risk)}</span>
            </div>
            <Show when=move || !findings.get().is_empty()>
                <div class="risk-finding-list">
                    <For each=move || findings.get() key=|finding| finding.code.clone() let:finding>
                        <div class=format!("risk-finding risk-{}", finding.level)>
                            <strong>{finding.code}</strong>
                            <span>{finding.message}</span>
                            <small>{finding.element}</small>
                        </div>
                    </For>
                </div>
            </Show>
            <div class="change-list">
                <For each=move || changes.get() key=|change| format!("{}:{}:{}", change.change_type, change.element, change.path) let:change>
                    <VersionChangeRow change=change />
                </For>
            </div>
            <label class="review-acknowledgement">
                <input
                    type="checkbox"
                    prop:checked=move || confirmed.get()
                    on:change=move |event| set_confirmed.set(event_target_checked(&event))
                />
                <span>"我已检查本次变更、风险提示和变更前后值，确认保存为新草稿版本并解锁后续修改"</span>
            </label>
            <div class="review-actions">
                <button
                    class="button button-quiet"
                    on:click=move |event| on_discard.run(event)
                    disabled=move || saving.get()
                >"放弃结果"</button>
                <button
                    class="button button-primary"
                    on:click=move |event| on_confirm.run(event)
                    disabled=move || saving.get() || !confirmed.get()
                >
                    {move || if saving.get() { "保存中..." } else { "确认并保存版本" }}
                </button>
            </div>
        </section>
    }
}

#[component]
pub fn SavedVersionReviewView(review: VersionChangeReview) -> impl IntoView {
    let changes = RwSignal::new(review.changes);
    let findings = RwSignal::new(review.findings);
    let risk_class = format!("review-risk risk-{}", review.overall_risk);
    view! {
        <div class="saved-version-review">
            <div class="saved-review-summary">
                <span class=risk_class>{format!("{}风险", review.overall_risk)}</span>
                <strong>{review.summary}</strong>
            </div>
            <Show when=move || !findings.get().is_empty()>
                <div class="risk-finding-list">
                    <For each=move || findings.get() key=|finding| finding.code.clone() let:finding>
                        <div class=format!("risk-finding risk-{}", finding.level)>
                            <strong>{finding.code}</strong><span>{finding.message}</span><small>{finding.element}</small>
                        </div>
                    </For>
                </div>
            </Show>
            <div class="change-list">
                <For each=move || changes.get() key=|change| format!("{}:{}:{}", change.change_type, change.element, change.path) let:change>
                    <VersionChangeRow change=change />
                </For>
            </div>
        </div>
    }
}

fn compact_review_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "未设置".into(),
        serde_json::Value::String(value) => value.clone(),
        _ => serde_json::to_string(value).unwrap_or_else(|_| "无法显示".into()),
    }
}

#[component]
pub fn VersionChangeRow(change: VersionDiffEntry) -> impl IntoView {
    let class = format!("change-row change-{}", change.change_type);
    let path = (!change.path.is_empty()).then_some(change.path);
    let before = compact_review_value(&change.before);
    let after = compact_review_value(&change.after);
    view! {
        <div class=class>
            <div class="change-row-title">
                <strong>{change.element}</strong>
                <span>{change.element_type}</span>
                <em>{change.change_type}</em>
            </div>
            {path.map(|path| view! { <code>{path}</code> })}
            <div class="change-values">
                <div><small>"变更前"</small><code>{before}</code></div>
                <div><small>"变更后"</small><code>{after}</code></div>
            </div>
            <small class="change-reason">{change.reason}</small>
        </div>
    }
}

#[component]
pub fn VerificationReportView(
    report: VersionVerificationReport,
    publish_confirmed: ReadSignal<bool>,
    set_publish_confirmed: WriteSignal<bool>,
    on_verify: impl Fn(web_sys::MouseEvent) + Send + Sync + 'static,
    on_publish: impl Fn(web_sys::MouseEvent) + Send + Sync + 'static,
    busy: ReadSignal<bool>,
    published: bool,
    on_review: impl Fn(web_sys::MouseEvent) + Send + Sync + 'static,
    on_view_change: impl Fn(web_sys::MouseEvent) + Send + Sync + 'static,
    action_error: ReadSignal<Option<String>>,
) -> impl IntoView {
    let on_verify = Callback::new(on_verify);
    let on_publish = Callback::new(on_publish);
    let on_review = Callback::new(on_review);
    let on_view_change = Callback::new(on_view_change);
    let passed = report.status == "passed";
    let high_risk_review_required = report.gates.iter().any(|gate| {
        gate.code == "CHANGE_REVIEW"
            && gate.status == "failed"
            && gate.message.starts_with("高风险变更尚未确认")
    });
    let gates = RwSignal::new(report.gates);
    let cases = RwSignal::new(report.cases);
    let boundary_cases_failed = cases
        .get_untracked()
        .iter()
        .any(|case| case.category == "boundary" && case.status != "passed");
    let cases_open = RwSignal::new(boundary_cases_failed);
    let findings = RwSignal::new(report.findings);
    let panel_class = format!(
        "verification-panel {}",
        if passed { "is-passed" } else { "is-failed" }
    );
    view! {
        <section class=panel_class aria-label="版本验收与发布">
            <div class="verification-heading">
                <div>
                    <span class="yaml-kicker">"RELEASE GATES"</span>
                    <strong>{if published { "已发布版本" } else { "自动验收与发布门禁" }}</strong>
                    <small>{format!("{} / {} 个案例通过，自动边界案例 {} 个", report.passed, report.total, report.boundary_cases)}</small>
                </div>
                <span class=if passed { "gate-summary gate-passed" } else { "gate-summary gate-failed" }>
                    {if passed { "门禁通过" } else { "门禁失败" }}
                </span>
            </div>
            <div class="verification-gates">
                <For each=move || gates.get() key=|gate| gate.code.clone() let:gate>
                    {
                        let is_high_risk_review_gate = high_risk_review_required
                            && gate.code == "CHANGE_REVIEW";
                        let is_boundary_failure = gate.code == "BOUNDARY_EXECUTION"
                            && gate.status == "failed";
                        view! {
                            <div class=format!("verification-gate gate-{}", gate.status)>
                                <strong>{gate.name}</strong>
                                <span>{match gate.status.as_str() { "passed" => "通过", "warning" => "警告", _ => "失败" }}</span>
                                <small>{gate.message}</small>
                                <Show when=move || is_high_risk_review_gate>
                                    <button
                                        class="button button-primary verification-review-action"
                                        on:click=move |event| on_review.run(event)
                                        disabled=busy
                                        title="需要由非版本创建者完成复核"
                                    >
                                        {move || if busy.get() { "确认中..." } else { "确认高风险变更" }}
                                    </button>
                                    <Show when=move || action_error.get().is_some()>
                                        <div class="verification-action-error" role="alert">
                                            <strong>"操作未完成"</strong>
                                            <span>{move || action_error.get().unwrap_or_default()}</span>
                                        </div>
                                    </Show>
                                </Show>
                                <Show when=move || is_boundary_failure>
                                    <button
                                        class="button button-primary verification-review-action"
                                        on:click=move |_| cases_open.set(true)
                                        title="查看失败边界案例的输入和执行错误"
                                    >
                                        "查看失败边界案例"
                                    </button>
                                </Show>
                            </div>
                        }
                    }
                </For>
            </div>
            <Show when=move || !cases.get().is_empty()>
                <details class="verification-cases" prop:open=move || cases_open.get()>
                    <summary>{format!("查看 {} 个验收案例详情", cases.get().len())}</summary>
                    <div class="verification-case-list">
                        <For each=move || cases.get() key=|case| case.name.clone() let:case>
                            <div class=format!("verification-case case-{}", case.status)>
                                <div>
                                    <strong>{case.name}</strong>
                                    <span>{match case.status.as_str() { "passed" => "通过", _ => "失败" }}</span>
                                </div>
                                <small>{format!(
                                    "输入：{}",
                                    serde_json::to_string(&case.input).unwrap_or_default()
                                )}</small>
                                <small>{format!("期望：{}", case.expectation)}</small>
                                <small>{format!("实际：{}", case.actual)}</small>
                            </div>
                        </For>
                    </div>
                </details>
            </Show>
            <Show when=move || !findings.get().is_empty()>
                <div class="verification-findings">
                    <strong>"分析发现"</strong>
                    <For each=move || findings.get()
                        key=|finding| format!(
                            "{}:{}:{}:{}",
                            finding.level, finding.code, finding.element, finding.message
                        )
                        let:finding
                    >
                        <div class=format!("verification-finding finding-{}", finding.level)>
                            <div>
                                <strong>{finding.code}</strong>
                                <span>{finding.message}</span>
                            </div>
                            <small>{finding.element}</small>
                        </div>
                    </For>
                </div>
            </Show>
            <Show when=move || !published>
                <div class="verification-actions">
                    <div class="verification-refresh-actions">
                        <button
                            class="button button-quiet"
                            on:click=move |event| on_verify.run(event)
                            disabled=busy
                        >
                            {move || if busy.get() { "验收中..." } else { "重新验收" }}
                        </button>
                        <small>"重新执行自动测试并刷新发布门禁"</small>
                    </div>
                    <Show when=move || passed>
                        <div class="verification-publish-actions">
                            <label class="publish-acknowledgement">
                                <input
                                    type="checkbox"
                                    prop:checked=move || publish_confirmed.get()
                                    on:change=move |event| set_publish_confirmed.set(event_target_checked(&event))
                                />
                                <span>"我已确认该版本通过所有验收门禁，可以发布"</span>
                            </label>
                            <button
                                class="button button-primary verification-publish-button"
                                on:click=move |event| on_publish.run(event)
                                disabled=move || busy.get() || !publish_confirmed.get()
                            >
                                {move || if busy.get() { "发布中..." } else { "确认发布" }}
                            </button>
                        </div>
                    </Show>
                </div>
            </Show>
            <Show when=move || !high_risk_review_required && action_error.get().is_some()>
                <div class="verification-action-error" role="alert">
                    <strong>"操作未完成"</strong>
                    <span>{move || action_error.get().unwrap_or_default()}</span>
                </div>
            </Show>
            <Show when=move || published>
                <button class="button button-quiet" on:click=move |event| on_view_change.run(event)>"查看变更详情"</button>
            </Show>
        </section>
    }
}
