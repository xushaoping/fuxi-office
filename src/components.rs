use std::collections::{BTreeMap, HashSet};

use gloo_storage::{LocalStorage, Storage};
use leptos::ev::KeyboardEvent;
use leptos::logging::{log, warn};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::{
    components::{A, Redirect, Route, Router, Routes},
    path,
};
use serde_yaml::Value as YamlValue;
use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::JsFuture;

use crate::api::{self, ChangeSummary, EvaluateResult, RunResult, SseEvent};
use crate::models::{
    AdminUser, AuditEvent, CreateDecisionProject, CreateDecisionTestCase, CreateDecisionVersion,
    CreateUser, DecisionAnalyticsReport, DecisionOptimizationProposal, DecisionProject,
    DecisionServiceApiKey, DecisionServiceRun, DecisionTestCase, DecisionVersion, ExecutionTrace,
    KnowledgeCitation, KnowledgeSource, ModelChangeIntent, ModelLibraryItem, ProjectMember,
    PublishedDecisionService, SaveDecisionRunOutcome, SaveDecisionTemplate, SaveKnowledgeSource,
    UpdateUser, VersionChangeReview, VersionDiffEntry, VersionVerificationReport,
};
use crate::workflow::{
    Clarification, ClarificationAnswer, DecisionBrief, DecisionField, DecisionPlan,
    DecisionWorkflowState,
};
use web_sys::AbortController;

#[derive(Clone, Debug)]
struct ChatMessage {
    role: &'static str,
    content: String,
}

#[component]
fn ClarificationInput(
    item: Clarification,
    answers: ReadSignal<BTreeMap<String, String>>,
    set_answers: WriteSignal<BTreeMap<String, String>>,
) -> impl IntoView {
    let id_for_value = item.id.clone();
    let id_for_change = item.id.clone();
    let control = if item.options.is_empty() {
        view! {
            <input
                type="text"
                placeholder="输入你的回答"
                prop:value=move || answers.get().get(&id_for_value).cloned().unwrap_or_default()
                on:input=move |event| {
                    let value = event_target_value(&event);
                    set_answers.update(|answers| { answers.insert(id_for_change.clone(), value); });
                }
            />
        }
        .into_any()
    } else {
        let options = RwSignal::new(item.options.clone());
        view! {
            <select
                prop:value=move || answers.get().get(&id_for_value).cloned().unwrap_or_default()
                on:change=move |event| {
                    let value = event_target_value(&event);
                    set_answers.update(|answers| { answers.insert(id_for_change.clone(), value); });
                }
            >
                <option value="">"请选择"</option>
                <For each=move || options.get() key=|option| option.clone() let:option>
                    <option value=option.clone()>{option.clone()}</option>
                </For>
            </select>
        }
        .into_any()
    };
    view! {
        <label class="clarification-item">
            <span>{item.question}</span>
            <small>{item.reason}</small>
            {control}
        </label>
    }
}

#[component]
fn WorkflowFieldRow(field: DecisionField) -> impl IntoView {
    let display_name = if field.label.is_empty() {
        field.name.clone()
    } else {
        field.label
    };
    let unit = if field.unit.is_empty() {
        None
    } else {
        Some(field.unit)
    };

    view! {
        <div class="workflow-field-row">
            <div>
                <strong>{display_name}</strong>
                <code>{field.name}</code>
            </div>
            <span>{field.type_ref}</span>
            <small>{field.description}</small>
            <div class="workflow-field-meta">
                <Show when=move || field.required><b>"必填"</b></Show>
                {unit.map(|value| view! { <em>{value}</em> })}
            </div>
        </div>
    }
}

#[component]
fn DecisionBriefView(brief: DecisionBrief) -> impl IntoView {
    let inputs = RwSignal::new(brief.inputs);
    let outputs = RwSignal::new(brief.outputs);
    let constraints = RwSignal::new(brief.constraints);
    let assumptions = RwSignal::new(brief.assumptions);
    let terms = RwSignal::new(brief.terms);
    let input_count = inputs.get_untracked().len();
    let output_count = outputs.get_untracked().len();

    view! {
        <div class="workflow-brief">
            <div>
                <span>"业务目标"</span>
                <strong>{brief.goal}</strong>
                <small>{brief.description}</small>
            </div>
            <div class="workflow-stat"><strong>{input_count}</strong><span>"输入"</span></div>
            <div class="workflow-stat"><strong>{output_count}</strong><span>"输出"</span></div>
            <div class="workflow-stat"><strong>{brief.risk_level}</strong><span>"风险"</span></div>
        </div>
        <div class="workflow-field-grid">
            <section>
                <strong>"输入字段"</strong>
                <Show
                    when=move || !inputs.get().is_empty()
                    fallback=|| view! { <small class="workflow-empty">"暂无输入字段"</small> }
                >
                    <For each=move || inputs.get() key=|field| field.name.clone() let:field>
                        <WorkflowFieldRow field=field />
                    </For>
                </Show>
            </section>
            <section>
                <strong>"输出字段"</strong>
                <Show
                    when=move || !outputs.get().is_empty()
                    fallback=|| view! { <small class="workflow-empty">"暂无输出字段"</small> }
                >
                    <For each=move || outputs.get() key=|field| field.name.clone() let:field>
                        <WorkflowFieldRow field=field />
                    </For>
                </Show>
            </section>
        </div>
        <Show when=move || !constraints.get().is_empty() || !assumptions.get().is_empty() || !terms.get().is_empty()>
            <div class="workflow-context-grid">
                <Show when=move || !constraints.get().is_empty()>
                    <section><strong>"约束"</strong><For each=move || constraints.get() key=|item| item.clone() let:item><span>{item}</span></For></section>
                </Show>
                <Show when=move || !assumptions.get().is_empty()>
                    <section><strong>"假设"</strong><For each=move || assumptions.get() key=|item| item.clone() let:item><span>{item}</span></For></section>
                </Show>
                <Show when=move || !terms.get().is_empty()>
                    <section><strong>"业务术语"</strong><For each=move || terms.get() key=|item| item.term.clone() let:item><span><b>{item.term}</b>{format!("：{}", item.meaning)}</span></For></section>
                </Show>
            </div>
        </Show>
    }
}

#[component]
fn RequirementAnalysisProgress(
    prompt: ReadSignal<String>,
    agent_activity: ReadSignal<String>,
) -> impl IntoView {
    view! {
        <section class="requirement-analysis-progress" aria-live="polite">
            <div class="analysis-progress-heading">
                <span class="analysis-progress-indicator" aria-hidden="true"><i></i><i></i><i></i></span>
                <div><strong>"正在分析需求"</strong><small>{move || agent_activity.get()}</small></div>
            </div>
            <p class="analysis-progress-prompt">{move || prompt.get()}</p>
            <div class="analysis-progress-steps">
                <span class="is-complete">"提取业务目标和决策范围"</span>
                <span class="is-active">"识别输入、输出和关键规则"</span>
                <span>"检查歧义并准备澄清问题"</span>
            </div>
        </section>
    }
}

#[component]
fn DecisionPlanView(plan: DecisionPlan) -> impl IntoView {
    let decisions = RwSignal::new(plan.decisions);
    let knowledge_models = RwSignal::new(plan.business_knowledge_models);
    let services = RwSignal::new(plan.decision_services);
    let defaults = RwSignal::new(plan.defaults);
    let criteria = RwSignal::new(plan.acceptance_criteria);
    view! {
        <div class="decision-plan-view">
            <p>{plan.summary}</p>
            <div class="plan-decision-list">
                <For each=move || decisions.get() key=|decision| decision.name.clone() let:decision>
                    <div><strong>{decision.name}</strong><span>{decision.expression_type}</span><small>{decision.purpose}</small><code>{decision.dependencies.join(" → ")}</code></div>
                </For>
            </div>
            <Show when=move || !knowledge_models.get().is_empty()>
                <div class="plan-secondary-list">
                    <strong>"业务知识模型"</strong>
                    <For each=move || knowledge_models.get() key=|model| model.name.clone() let:model>
                        <div>
                            <strong>{model.name}</strong><span>{model.output_type}</span>
                            <small>{model.purpose}</small>
                            <code>{format!("参数：{}", model.parameters.join("、"))}</code>
                        </div>
                    </For>
                </div>
            </Show>
            <Show when=move || !services.get().is_empty()>
                <div class="plan-secondary-list">
                    <strong>"决策服务"</strong>
                    <For each=move || services.get() key=|service| service.name.clone() let:service>
                        <div>
                            <strong>{service.name}</strong>
                            <small>{format!("输入：{}", service.input_data.join("、"))}</small>
                            <code>{format!("封装：{}；输出：{}", service.encapsulated_decisions.join("、"), service.output_decisions.join("、"))}</code>
                        </div>
                    </For>
                </div>
            </Show>
            <Show when=move || !defaults.get().is_empty()>
                <div class="acceptance-list plan-defaults"><strong>"默认策略"</strong><For each=move || defaults.get() key=|item| item.clone() let:item><span>{item}</span></For></div>
            </Show>
            <Show when=move || !criteria.get().is_empty()>
                <div class="acceptance-list"><strong>"验收标准"</strong><For each=move || criteria.get() key=|item| item.clone() let:item><span>{item}</span></For></div>
            </Show>
        </div>
    }
}

#[component]
fn VersionChangeReviewView(
    review: VersionChangeReview,
    confirmed: ReadSignal<bool>,
    set_confirmed: WriteSignal<bool>,
    on_confirm: impl Fn(leptos::ev::MouseEvent) + Copy + 'static,
    on_discard: impl Fn(leptos::ev::MouseEvent) + Copy + 'static,
    saving: ReadSignal<bool>,
) -> impl IntoView {
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
                <button class="button button-quiet" on:click=on_discard disabled=move || saving.get()>"放弃结果"</button>
                <button class="button button-primary" on:click=on_confirm disabled=move || saving.get() || !confirmed.get()>
                    {move || if saving.get() { "保存中..." } else { "确认并保存版本" }}
                </button>
            </div>
        </section>
    }
}

#[component]
fn SavedVersionReviewView(review: VersionChangeReview) -> impl IntoView {
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
fn VersionChangeRow(change: VersionDiffEntry) -> impl IntoView {
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
fn VerificationReportView(
    report: VersionVerificationReport,
    publish_confirmed: ReadSignal<bool>,
    set_publish_confirmed: WriteSignal<bool>,
    on_verify: impl Fn(leptos::ev::MouseEvent) + Copy + 'static,
    on_publish: impl Fn(leptos::ev::MouseEvent) + Copy + 'static,
    busy: ReadSignal<bool>,
    published: bool,
    on_review: impl Fn(leptos::ev::MouseEvent) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let passed = report.status == "passed";
    let gates = RwSignal::new(report.gates);
    let cases = RwSignal::new(report.cases);
    let findings = RwSignal::new(report.findings);
    view! {
        <section class="verification-panel" aria-label="版本验收与发布">
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
                    <div class=format!("verification-gate gate-{}", gate.status)>
                        <strong>{gate.name}</strong>
                        <span>{match gate.status.as_str() { "passed" => "通过", "warning" => "警告", _ => "失败" }}</span>
                        <small>{gate.message}</small>
                    </div>
                </For>
            </div>
            <Show when=move || !published && gates.get().iter().any(|gate| gate.code == "CHANGE_REVIEW" && gate.status == "failed")>
                <div class="verification-review-callout" role="alert">
                    <div>
                        <strong>"高风险变更需要独立复核"</strong>
                        <span>"请由非版本创建者的复核者完成复核，完成后重新执行自动验收。"</span>
                    </div>
                    <button class="button button-quiet" on:click=on_review disabled=move || busy.get()>
                        {move || if busy.get() { "处理中..." } else { "独立复核当前版本" }}
                    </button>
                </div>
            </Show>
            <div class="verification-findings">
                <For each=move || findings.get() key=|finding| format!("{}:{}:{}", finding.code, finding.element, finding.rule_ids.join(",")) let:finding>
                    <div class=format!("verification-finding finding-{}", finding.level)>
                        <div><strong>{finding.code}</strong><span>{finding.element}</span></div>
                        <small>{finding.message}</small>
                        <code>{if finding.rule_ids.is_empty() { "模型级".into() } else { format!("规则：{}", finding.rule_ids.join("、")) }}</code>
                    </div>
                </For>
            </div>
            <details class="verification-cases">
                <summary>{format!("查看 {} 个自动验收案例", report.total)}</summary>
                <div class="verification-case-list">
                    <For each=move || cases.get() key=|case| format!("{}:{}", case.category, case.name) let:case>
                        <div class=format!("verification-case case-{}", case.status)>
                            <div><strong>{case.name}</strong><span>{case.category}</span></div>
                            <small>{format!("{}；实际：{}", case.expectation, case.actual)}</small>
                            <code>{serde_json::to_string(&case.input).unwrap_or_default()}</code>
                        </div>
                    </For>
                </div>
            </details>
            <div class=if published { "verification-publish-actions is-hidden" } else { "verification-publish-actions" }>
                <label class="review-acknowledgement">
                    <input
                        type="checkbox"
                        prop:checked=move || publish_confirmed.get()
                        on:change=move |event| set_publish_confirmed.set(event_target_checked(&event))
                    />
                    <span>"我已检查自动验收结果，确认将当前版本作为项目的正式发布版本"</span>
                </label>
                <small class="publish-identity-hint">"发布审批必须由未创建当前版本、且未执行独立复核的审批人账号完成；同一账号拥有多个角色不会绕过此门禁。"</small>
                <div class="review-actions">
                    <button class="button button-quiet" on:click=on_verify disabled=move || busy.get()>
                        {move || if busy.get() { "处理中..." } else { "重新验收" }}
                    </button>
                    <button class="button button-primary" on:click=on_publish disabled=move || busy.get() || !passed || !publish_confirmed.get()>
                        "确认发布"
                    </button>
                </div>
            </div>
        </section>
    }
}

#[component]
fn ExecutionTraceView(trace: ExecutionTrace) -> impl IntoView {
    let decisions = RwSignal::new(trace.decisions);
    let messages = RwSignal::new(trace.messages);
    let normalized_input =
        serde_json::to_string_pretty(&trace.normalized_input).unwrap_or_default();
    let output = serde_json::to_string_pretty(&trace.output).unwrap_or_default();
    let status = trace.status.clone();
    let status_class = format!("trace-status trace-status-{status}");
    view! {
        <section class="execution-trace" aria-label="确定性执行轨迹">
            <div class="execution-trace-heading">
                <div>
                    <span class="yaml-kicker">"DETERMINISTIC TRACE"</span>
                    <strong>{format!("{}{}", trace.model_name, trace.version_number.map(|number| format!(" · v{number}")).unwrap_or_default())}</strong>
                    <small>{format!("{} 个决策步骤 · {status}", decisions.get_untracked().len())}</small>
                </div>
                <span class=status_class>{status}</span>
            </div>
            <p class="trace-explanation">{trace.explanation}</p>
            <div class="trace-contract">
                <details class="trace-contract-item trace-contract-input"><summary>"标准化输入"</summary><code>{normalized_input}</code></details>
                <details class="trace-contract-item trace-contract-output"><summary>"完整输出"</summary><code>{output}</code></details>
            </div>
            <div class="trace-steps">
                <For each=move || decisions.get() key=|step| format!("{}:{}", step.order, step.decision_id) let:step>
                    {let dependencies = step.dependencies.clone();
                    let dependency_text = dependencies.join("、");
                    let has_dependencies = !dependencies.is_empty();
                    let rule_ids = step.matched_rule_ids.clone();
                    let rule_text = rule_ids.join("、");
                    let has_rules = !rule_ids.is_empty();
                    view! {
                    <article class=format!("trace-step trace-outcome-{}", step.outcome)>
                        <div class="trace-step-index">{format!("{:02}", step.order)}</div>
                        <div class="trace-step-body">
                            <div class="trace-step-heading">
                                <strong>{step.decision_name}</strong>
                                <span>{step.logic_type.replace('_', " ")}</span>
                                <em>{match step.outcome.as_str() {
                                    "rule_match" => "规则命中",
                                    "default_output" => "默认输出",
                                    "no_match" => "未命中",
                                    "failed" => "失败",
                                    "skipped" => "跳过",
                                    _ => "表达式求值",
                                }}</em>
                            </div>
                            <p>{step.explanation}</p>
                            <div class="trace-step-meta">
                                <Show when=move || has_dependencies>
                                    <small>{format!("依赖：{dependency_text}")}</small>
                                </Show>
                                <Show when=move || has_rules>
                                    <small>{format!("规则：{rule_text}")}</small>
                                </Show>
                            </div>
                            <code>{serde_json::to_string_pretty(&step.result).unwrap_or_default()}</code>
                        </div>
                    </article>
                    }}
                </For>
            </div>
            <Show when=move || !messages.get().is_empty()>
                <div class="trace-messages">
                    <For each=move || messages.get() key=|message| format!("{}:{}:{}", message.severity, message.code, message.message) let:message>
                        <div><span>{message.severity}</span><code>{message.code}</code><p>{message.message}</p></div>
                    </For>
                </div>
            </Show>
        </section>
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunStatus {
    Idle,
    Running,
    Succeeded,
    Failed,
    Aborted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GenerationMode {
    Create,
    Modify,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkspaceView {
    Discovery,
    Model,
    Validation,
    Services,
    Analytics,
    Versions,
    Collaboration,
    Resources,
}

const MAX_DMN_COMPILE_REPAIRS: i32 = 2;

fn is_repairable_model_change_error(message: &str) -> bool {
    message.starts_with("[DMN_COMPILE]")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResourceView {
    Knowledge,
    Models,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Theme {
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CopyStatus {
    Idle,
    Copied,
    Failed,
}

impl Theme {
    fn class(self) -> &'static str {
        match self {
            Self::Dark => "theme-dark",
            Self::Light => "theme-light",
        }
    }

    fn toggle_label(self) -> &'static str {
        match self {
            Self::Dark => "切换到浅色主题",
            Self::Light => "切换到深色主题",
        }
    }
}

impl RunStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Idle => "准备就绪",
            Self::Running => "智能体运行中",
            Self::Succeeded => "已完成",
            Self::Failed => "运行失败",
            Self::Aborted => "已停止",
        }
    }

    fn class(self) -> &'static str {
        match self {
            Self::Idle => "status-idle",
            Self::Running => "status-running",
            Self::Succeeded => "status-success",
            Self::Failed => "status-error",
            Self::Aborted => "status-muted",
        }
    }
}

fn agent_activity_for_step(step: &str) -> String {
    if step.contains("等待首段输出") {
        "正在等待模型返回首段内容".to_string()
    } else if step.contains("ChangeIntent") || step.contains("变更意图") {
        "正在生成并校验受控模型变更意图".to_string()
    } else if step.contains("调用 LLM") {
        "正在调用语言模型生成 DSL".to_string()
    } else if step.contains("校验") {
        "正在校验 DSL 与 DMN 结构".to_string()
    } else if step.contains("编译") {
        "正在编译 DMN XML".to_string()
    } else if step.contains("重试") {
        "正在修复生成结果并重新尝试".to_string()
    } else {
        step.trim_start_matches(['→', '✓', '⚠', '…', ' '])
            .to_string()
    }
}

fn timeline_item_class(step: &str, is_current: bool) -> &'static str {
    if step.starts_with('✓') {
        "is-complete"
    } else if step.starts_with('⚠') {
        "is-warning"
    } else if step.starts_with('…') {
        "is-waiting"
    } else if is_current {
        "is-active"
    } else {
        "is-past"
    }
}

fn timeline_item_text(step: &str) -> String {
    step.trim_start_matches(['→', '✓', '⚠', '…', ' '])
        .to_string()
}

fn workflow_status_label(status: &str) -> &'static str {
    match status {
        "analyzed" => "已分析",
        "clarification" => "待澄清",
        "planned" => "待确认",
        "confirmed" => "已确认",
        _ => "待分析",
    }
}

fn service_sample_value(type_ref: &str) -> serde_json::Value {
    match type_ref
        .strip_prefix("feel:")
        .unwrap_or(type_ref)
        .to_ascii_lowercase()
        .as_str()
    {
        // Published service templates should be runnable out of the box;
        // zero would make a denominator-based decision fail immediately.
        "number" => serde_json::json!(1),
        "boolean" => serde_json::json!(false),
        "list" => serde_json::json!([]),
        _ => serde_json::json!(""),
    }
}

fn service_input_template(service: &PublishedDecisionService) -> String {
    serde_json::to_string_pretty(
        &service
            .inputs
            .iter()
            .map(|field| (field.name.clone(), service_sample_value(&field.type_ref)))
            .collect::<serde_json::Map<_, _>>(),
    )
    .unwrap_or_else(|_| "{}".into())
}

fn recent_service_runs(runs: Vec<DecisionServiceRun>) -> Vec<DecisionServiceRun> {
    runs.into_iter().take(10).collect()
}

fn outcome_assessment_label(assessment: Option<&str>) -> &'static str {
    match assessment {
        Some("confirmed") => "符合决策",
        Some("mismatch") => "存在偏差",
        Some("inconclusive") => "无法判断",
        _ => "待回填",
    }
}

fn analytics_percent(rate: f64) -> String {
    format!("{:.1}%", rate * 100.0)
}

fn analytics_finding_label(code: &str) -> &'static str {
    match code {
        "NO_RECENT_RUNS" => "暂无运行数据",
        "HIGH_FAILURE_RATE" => "失败率偏高",
        "LOW_FEEDBACK_COVERAGE" => "反馈覆盖不足",
        "HIGH_MISMATCH_RATE" => "结果偏差偏高",
        "LOW_RULE_COVERAGE" => "规则覆盖不足",
        "FREQUENT_FALLBACK" => "频繁未命中规则",
        "HIGH_P95_LATENCY" => "长尾耗时偏高",
        "FAILURE_TREND_UP" => "失败趋势上升",
        "FEEDBACK_TREND_DOWN" => "反馈趋势下降",
        "LATENCY_TREND_UP" => "耗时趋势上升",
        _ => "运行异常",
    }
}

fn optimization_proposal_actions(proposal: &DecisionOptimizationProposal) -> Vec<String> {
    proposal
        .proposed_actions
        .as_array()
        .map(|actions| {
            actions
                .iter()
                .filter_map(|action| action.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn optimization_proposal_status_label(status: &str) -> &'static str {
    match status {
        "open" => "待确认",
        "confirmed" => "已建草稿",
        "dismissed" => "已驳回",
        _ => "未知状态",
    }
}

fn public_service_endpoint(service: &PublishedDecisionService) -> String {
    let origin = web_sys::window()
        .and_then(|window| window.location().origin().ok())
        .filter(|origin| !origin.is_empty())
        .unwrap_or_default();
    format!(
        "{origin}/api/public/projects/{}/decision-services/execute",
        service.project_id
    )
}

fn public_service_curl(service: &PublishedDecisionService, secret: &str) -> String {
    let inputs = service_input_template(service);
    let mut command = format!("curl -X POST '{}'", public_service_endpoint(service));
    command.push_str(&format!(" \\\n  -H 'X-Decision-Key: {}'", secret));
    command.push_str(" \\\n  -H 'Content-Type: application/json'");
    command.push_str(&format!(
        " \\\n  -d '{{\n    \"serviceId\": \"{}\",\n    \"businessReference\": \"ORDER-20260903-001\",\n    \"inputs\": {}\n  }}'",
        service.id, inputs
    ));
    command
}

fn timeline_entries(steps: Vec<String>) -> Vec<(usize, String)> {
    steps.into_iter().enumerate().collect()
}

const TOKEN_KEY: &str = "auth_token";

// Token 信号由 App 根组件创建并通过 Context 共享（见 App）：
// 不能在子组件 owner 中创建，否则路由切换卸载组件时会连带 dispose 信号，
// 导致后续 token.get() panic（reactive value already disposed）
fn auth_token() -> RwSignal<Option<String>> {
    use_context::<RwSignal<Option<String>>>().expect("auth_token 必须在 <App> 组件内使用")
}

// ==========================================
// 1. App 根组件: 路由配置
// ==========================================
#[component]
pub fn App() -> impl IntoView {
    // 根组件 owner 随应用存活，不会因路由切换被 dispose，
    // 登录后 set(Some) 会同步触发 RequireAuth/Dashboard 的响应式更新
    let token = RwSignal::new(LocalStorage::get::<String>(TOKEN_KEY).ok());
    provide_context(token);

    view! {
        <Router>
            <AuthExpiryWatcher />
            <Routes fallback=|| view! { <p>"加载中..."</p> }>
                <Route path=path!("/") view=move || view! { <RequireAuth><DmnApp /></RequireAuth> } />
                <Route path=path!("/admin") view=move || view! { <RequireAuth><AdminUsersPage /></RequireAuth> } />
                <Route path=path!("/login") view=LoginPage />
            </Routes>
        </Router>
    }
}

#[component]
fn AuthExpiryWatcher() -> impl IntoView {
    let token = auth_token();
    let navigate = leptos_router::hooks::use_navigate();
    let window = web_sys::window().expect("浏览器窗口不可用");
    let callback = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        LocalStorage::delete(TOKEN_KEY);
        token.set(None);
        navigate("/login", Default::default());
    });
    let callback_ref = callback.as_ref().unchecked_ref();
    window
        .add_event_listener_with_callback("auth-expired", callback_ref)
        .expect("无法注册登录过期监听器");
    callback.forget();

    view! {}
}

#[component]
fn RequireAuth(children: ChildrenFn) -> impl IntoView {
    let token = auth_token();
    move || {
        if token.get().is_some() {
            children().into_any()
        } else {
            view! { <Redirect path="/login"/> }.into_any()
        }
    }
}

#[component]
fn LoginPage() -> impl IntoView {
    let token = auth_token();
    let navigate = leptos_router::hooks::use_navigate();
    let username = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let error_msg = RwSignal::new(None::<String>);
    let is_loading = RwSignal::new(false);

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default(); // 阻止表单默认提交导致页面刷新

        if is_loading.get() {
            return;
        } // 防止重复提交
        is_loading.set(true);
        error_msg.set(None);

        let username = username.get();
        let password = password.get();
        let navigate = navigate.clone();

        spawn_local(async move {
            log!("开始发送登录请求...");
            match api::login(&username, &password).await {
                Some(t) => {
                    LocalStorage::set(TOKEN_KEY, t.clone()).ok();
                    token.set(Some(t));
                    navigate("/", Default::default());
                }
                None => {
                    warn!("登录失败：用户名或密码错误");
                    error_msg.set(Some("用户名或密码错误".to_string()));
                    is_loading.set(false);
                }
            }
        });
    };

    view! {
        <div class="auth-shell">
            <div class="auth-card">
                <div class="auth-brand">
                    <div class="brand-mark">"D"</div>
                    <div>
                        <p class="eyebrow">"DMN DECISION WORKSPACE"</p>
                        <h1>"伏羲智能决策"</h1>
                    </div>
                </div>
                <div class="auth-heading">
                    <h2 class="title">"欢迎登录"</h2>
                    <p>"进入你的决策建模工作台"</p>
                </div>
            </div>
            <form on:submit=on_submit>
                <div class="form-group">
                    <input
                        type="text"
                        class="input"
                        placeholder="用户名"
                        bind:value=username
                        disabled=move || is_loading.get()
                    />
                </div>
                <div class="form-group">
                    <input
                        type="password"
                        class="input"
                        placeholder="密码"
                        bind:value=password
                        disabled=move || is_loading.get()
                    />
                </div>
                <button
                    type="submit"
                    class="btn btn-primary"
                    style="width: 100%;"
                    disabled=move || is_loading.get()
                >
                    {move || if is_loading.get() {
                        "处理中..."
                    } else {
                        "登 录"
                    }}
                </button>

                {move || error_msg.get().map(|e| view! {
                    <div class="error-msg">{e}</div>
                })}
            </form>
        </div>
    }
}

#[component]
fn AdminUsersPage() -> impl IntoView {
    let token = auth_token();
    let navigate = leptos_router::hooks::use_navigate();
    let users = RwSignal::new(Vec::<AdminUser>::new());
    let username = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let role = RwSignal::new("user".to_string());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let status = RwSignal::new(None::<String>);

    let reload = move || {
        let Some(token_value) = token.get() else {
            return;
        };
        spawn_local(async move {
            match api::list_admin_users(&token_value).await {
                Ok(items) => users.set(items),
                Err(message) => error.set(Some(message)),
            }
        });
    };
    Effect::new(move |_| {
        if token.get().is_some() {
            reload();
        }
    });

    let create = move |_| {
        let Some(token_value) = token.get() else {
            return;
        };
        let name = username.get().trim().to_string();
        let secret = password.get();
        if name.is_empty() || secret.len() < 6 || busy.get() {
            return;
        }
        busy.set(true);
        error.set(None);
        let selected_role = role.get();
        spawn_local(async move {
            match api::create_admin_user(
                &token_value,
                CreateUser {
                    username: name,
                    password: secret,
                    role: selected_role,
                },
            )
            .await
            {
                Ok(user) => {
                    users.update(|items| items.push(user));
                    username.set(String::new());
                    password.set(String::new());
                    status.set(Some("用户已添加".into()));
                }
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    let update_role = move |user: AdminUser, new_role: String| {
        let Some(token_value) = token.get() else {
            return;
        };
        if busy.get() || user.username == "admin" && new_role != "admin" {
            return;
        }
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match api::update_admin_user(
                &token_value,
                &user.username,
                UpdateUser {
                    role: new_role,
                    password: None,
                },
            )
            .await
            {
                Ok(updated) => users.update(|items| {
                    if let Some(item) = items
                        .iter_mut()
                        .find(|item| item.username == updated.username)
                    {
                        *item = updated;
                    }
                }),
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    let delete_user = move |username: String| {
        let Some(token_value) = token.get() else {
            return;
        };
        if busy.get() || username == "admin" {
            return;
        }
        if !web_sys::window()
            .and_then(|win| {
                win.confirm_with_message(&format!(
                    "确定要删除用户 \"{}\" 吗？此操作无法撤销。",
                    username
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
            match api::delete_admin_user(&token_value, &username).await {
                Ok(()) => {
                    users.update(|items| items.retain(|item| item.username != username));
                    status.set(Some("用户已删除".into()));
                }
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    view! {
        <main class="admin-page">
            <header class="admin-topbar">
                <div class="admin-brand">
                    <div class="brand-mark">"D"</div>
                    <div>
                        <span class="eyebrow">"DMN DECISION WORKSPACE"</span>
                        <strong>"系统管理"</strong>
                    </div>
                </div>
                <button class="button button-quiet" on:click=move |_| navigate("/", Default::default())>
                    <span aria-hidden="true">"←"</span> "返回工作台"
                </button>
            </header>
            <div class="admin-content">
                <section class="admin-heading">
                    <div>
                        <span class="yaml-kicker">"USER DIRECTORY / ACCESS CONTROL"</span>
                        <h1>"用户与角色"</h1>
                        <p>"集中管理工作台账号，并为每位成员分配系统级访问角色。"</p>
                    </div>
                    <div class="admin-summary">
                        <span class="admin-summary-label">"当前账号"</span>
                        <strong>"admin"</strong>
                        <span class="admin-role-badge admin-role-badge-primary">"超级管理员"</span>
                    </div>
                </section>
                <div class="admin-layout">
                    <section class="admin-user-editor">
                        <div class="admin-section-heading">
                            <div><span class="yaml-kicker">"ADD ACCOUNT"</span><h2>"添加用户"</h2></div>
                            <span class="admin-section-index">"01"</span>
                        </div>
                        <p class="admin-section-note">"创建后，用户可使用账号登录并参与已分配的项目。"</p>
                        <div class="admin-form-grid">
                            <label><span>"用户名"</span><input maxlength="255" placeholder="输入用户名" prop:value=username on:input=move |event| username.set(event_target_value(&event)) /></label>
                            <label><span>"初始密码"</span><input type="password" placeholder="至少 6 位" prop:value=password on:input=move |event| password.set(event_target_value(&event)) /></label>
                            <label><span>"系统角色"</span><select prop:value=role on:change=move |event| role.set(event_target_value(&event))><option value="user">"普通用户"</option><option value="admin">"超级管理员"</option></select></label>
                        </div>
                        <button class="button button-primary admin-submit" on:click=create disabled=move || busy.get() || username.get().trim().is_empty() || password.get().len() < 6>
                            <span aria-hidden="true">"+"</span> {move || if busy.get() { "处理中..." } else { "创建用户" }}
                        </button>
                    </section>
                    <section class="admin-users-panel">
                        <div class="admin-section-heading">
                            <div><span class="yaml-kicker">"ACTIVE ACCOUNTS"</span><h2>"用户列表"</h2></div>
                            <span class="admin-user-count">{move || users.get().len()} " 个用户"</span>
                        </div>
                        <div class="admin-list-header"><span>"用户"</span><span>"系统角色"</span><span>"操作"</span></div>
                        <div class="admin-user-list">
                            <For each=move || users.get() key=|user| user.username.clone() let:user>
                                {
                                    let role_value = user.role.clone();
                                    let role_label = if role_value == "admin" { "超级管理员" } else { "普通用户" };
                                    let role_class = if role_value == "admin" { "admin-role-badge admin-role-badge-primary" } else { "admin-role-badge" };
                                    let username_for_delete = user.username.clone();
                                    let username_for_select = user.username.clone();
                                    let username_for_disabled = user.username.clone();
                                    let username_for_title = user.username.clone();
                                    let is_admin = user.username == "admin";
                                    view! {
                                        <article class="admin-user-row">
                                            <div class="admin-user-identity">
                                                <span class="admin-avatar">{user.username.chars().next().unwrap_or('?').to_uppercase().to_string()}</span>
                                                <div><strong>{user.username.clone()}</strong><small>{format!("加入于 {}", user.created_at.format("%Y-%m-%d"))}</small></div>
                                            </div>
                                            <div class="admin-user-role">
                                                <span class=role_class>{role_label}</span>
                                                <select aria-label="修改系统角色" prop:value=role_value on:change={let user = user.clone(); move |event| update_role(user.clone(), event_target_value(&event))} disabled=move || username_for_select == "admin" || busy.get()>
                                                    <option value="user">"普通用户"</option>
                                                    <option value="admin">"超级管理员"</option>
                                                </select>
                                            </div>
                                            <div class="admin-user-actions">
                                                <button
                                                    class="button button-danger button-small"
                                                    on:click=move |_| delete_user(username_for_delete.clone())
                                                    disabled=move || username_for_disabled == "admin" || busy.get()
                                                    title=move || if username_for_title == "admin" { "不能删除 admin 用户" } else { "删除用户" }
                                                >
                                                    "删除"
                                                </button>
                                            </div>
                                        </article>
                                    }
                                }
                            </For>
                            <Show when=move || users.get().is_empty()><div class="admin-empty">"暂无用户记录"</div></Show>
                        </div>
                    </section>
                </div>
                <Show when=move || status.get().is_some()><div class="member-status">{move || status.get().unwrap_or_default()}</div></Show>
                <Show when=move || error.get().is_some()><div class="error-msg">{move || error.get().unwrap_or_default()}</div></Show>
            </div>
        </main>
    }
}

fn knowledge_type_label(value: &str) -> &'static str {
    match value {
        "term" => "企业术语",
        "field" => "字段目录",
        "policy" => "制度政策",
        _ => "知识源",
    }
}

fn audit_event_kind(event_type: &str) -> &'static str {
    match event_type {
        "test_cases.generated" | "test_case.approved" | "test_case.approval_revoked" => "test-case",
        "version.verified" => "verification",
        "version.published" => "published",
        "version.retired" => "retired",
        _ => "general",
    }
}

fn audit_event_type_label(event_type: &str) -> &'static str {
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

fn knowledge_effective_label(source: &KnowledgeSource) -> &'static str {
    if source.status == "archived" {
        "已归档"
    } else if source.is_effective {
        "当前有效"
    } else {
        "非当前有效"
    }
}

#[component]
fn KnowledgeLibraryPanel(
    token: RwSignal<Option<String>>,
    current_project: ReadSignal<Option<DecisionProject>>,
) -> impl IntoView {
    let sources = RwSignal::new(Vec::<KnowledgeSource>::new());
    let attached_ids = RwSignal::new(HashSet::<i64>::new());
    let selected_id = RwSignal::new(None::<i64>);
    let source_type = RwSignal::new("term".to_string());
    let name = RwSignal::new(String::new());
    let description = RwSignal::new(String::new());
    let content = RwSignal::new(String::new());
    let version_label = RwSignal::new(String::new());
    let source_uri = RwSignal::new(String::new());
    let valid_from = RwSignal::new(String::new());
    let valid_until = RwSignal::new(String::new());
    let filter = RwSignal::new("all".to_string());
    let query = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);

    let clear_form = move || {
        selected_id.set(None);
        source_type.set("term".into());
        name.set(String::new());
        description.set(String::new());
        content.set(String::new());
        version_label.set(String::new());
        source_uri.set(String::new());
        valid_from.set(String::new());
        valid_until.set(String::new());
    };

    Effect::new(move |_| {
        let Some(token_value) = token.get() else {
            sources.set(Vec::new());
            attached_ids.set(HashSet::new());
            return;
        };
        let project_id = current_project.get().map(|project| project.id);
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match api::list_knowledge_sources(&token_value).await {
                Ok(items) => sources.set(items),
                Err(message) => error.set(Some(message)),
            }
            if let Some(project_id) = project_id {
                match api::list_project_knowledge_sources(&token_value, project_id).await {
                    Ok(items) => attached_ids.set(items.into_iter().map(|item| item.id).collect()),
                    Err(message) => error.set(Some(message)),
                }
            } else {
                attached_ids.set(HashSet::new());
            }
            busy.set(false);
        });
    });

    let edit_source = move |source: KnowledgeSource| {
        selected_id.set(Some(source.id));
        source_type.set(source.source_type);
        name.set(source.name);
        description.set(source.description);
        content.set(source.content);
        version_label.set(source.version_label);
        source_uri.set(source.source_uri);
        valid_from.set(
            source
                .valid_from
                .map(|date| date.to_string())
                .unwrap_or_default(),
        );
        valid_until.set(
            source
                .valid_until
                .map(|date| date.to_string())
                .unwrap_or_default(),
        );
    };

    let save_source = move |_| {
        let Some(token_value) = token.get() else {
            return;
        };
        let parse_date = |value: String| {
            if value.trim().is_empty() {
                Ok(None)
            } else {
                chrono::NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d")
                    .map(Some)
                    .map_err(|_| "有效期必须使用 YYYY-MM-DD 格式".to_string())
            }
        };
        let valid_from_value = match parse_date(valid_from.get()) {
            Ok(value) => value,
            Err(message) => {
                error.set(Some(message));
                return;
            }
        };
        let valid_until_value = match parse_date(valid_until.get()) {
            Ok(value) => value,
            Err(message) => {
                error.set(Some(message));
                return;
            }
        };
        let payload = SaveKnowledgeSource {
            source_type: source_type.get(),
            name: name.get(),
            description: description.get(),
            content: content.get(),
            version_label: version_label.get(),
            source_uri: source_uri.get(),
            valid_from: valid_from_value,
            valid_until: valid_until_value,
            status: "active".into(),
        };
        if payload.name.trim().is_empty() || payload.content.trim().is_empty() {
            error.set(Some("知识名称和内容不能为空".into()));
            return;
        }
        let editing = selected_id.get();
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            let result = match editing {
                Some(id) => api::update_knowledge_source(&token_value, id, &payload).await,
                None => api::create_knowledge_source(&token_value, &payload).await,
            };
            match result {
                Ok(saved) => {
                    sources.update(|items| {
                        if let Some(item) = items.iter_mut().find(|item| item.id == saved.id) {
                            *item = saved.clone();
                        } else {
                            items.insert(0, saved);
                        }
                    });
                    clear_form();
                }
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    let archive_source = move |source_id: i64| {
        let Some(token_value) = token.get() else {
            return;
        };
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match api::archive_knowledge_source(&token_value, source_id).await {
                Ok(()) => {
                    sources.update(|items| {
                        if let Some(item) = items.iter_mut().find(|item| item.id == source_id) {
                            item.status = "archived".into();
                            item.is_effective = false;
                        }
                    });
                    attached_ids.update(|ids| {
                        ids.remove(&source_id);
                    });
                    if selected_id.get() == Some(source_id) {
                        clear_form();
                    }
                }
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    let toggle_attachment = move |source_id: i64, attached: bool| {
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            error.set(Some("请先选择决策项目".into()));
            return;
        };
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match api::set_project_knowledge_source(&token_value, project.id, source_id, attached)
                .await
            {
                Ok(()) => attached_ids.update(|ids| {
                    if attached {
                        ids.insert(source_id);
                    } else {
                        ids.remove(&source_id);
                    }
                }),
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    view! {
        <section class="knowledge-library-panel" aria-label="企业知识库">
            <div class="knowledge-library-heading">
                <div>
                    <span class="yaml-kicker">"GOVERNED KNOWLEDGE"</span>
                    <strong>"企业知识库"</strong>
                    <small>{move || format!("{} 个知识源 · 当前项目已绑定 {} 个", sources.get().len(), attached_ids.get().len())}</small>
                </div>
                <div class="knowledge-toolbar">
                    <input
                        class="resource-search"
                        type="search"
                        placeholder="搜索名称、说明或内容"
                        prop:value=query
                        on:input=move |event| query.set(event_target_value(&event))
                    />
                    <div class="knowledge-filters">
                        <For each=move || vec![("all", "全部"), ("attached", "已绑定"), ("term", "术语"), ("field", "字段"), ("policy", "制度")] key=|item| item.0 let:item>
                            <button class=move || if filter.get() == item.0 { "knowledge-filter is-active" } else { "knowledge-filter" } on:click=move |_| filter.set(item.0.into())>{item.1}</button>
                        </For>
                    </div>
                </div>
            </div>
            <div class="knowledge-library-body">
                <div class="knowledge-source-browser">
                    <div class="resource-column-heading">
                        <strong>"知识目录"</strong>
                        <button class="button button-quiet" on:click=move |_| clear_form()>"新建"</button>
                    </div>
                    <div class="knowledge-source-list">
                    <Show when=move || busy.get() && sources.get().is_empty()>
                        <div class="knowledge-empty">"正在读取知识库..."</div>
                    </Show>
                    <For
                        each=move || {
                            let selected = filter.get();
                            let search = query.get().trim().to_lowercase();
                            sources.get().into_iter().filter(|source| {
                                (selected == "all"
                                    || (selected == "attached" && attached_ids.get().contains(&source.id))
                                    || source.source_type == selected)
                                    && (search.is_empty()
                                        || source.name.to_lowercase().contains(&search)
                                        || source.description.to_lowercase().contains(&search)
                                        || source.content.to_lowercase().contains(&search))
                            }).collect::<Vec<_>>()
                        }
                        key=|source| format!("{}:{}:{}", source.id, source.status, source.updated_at)
                        let:source
                    >
                        {
                            let source_for_edit = source.clone();
                            let source_id = source.id;
                            let archived = source.status == "archived";
                            let validity = match (source.valid_from, source.valid_until) {
                                (Some(from), Some(until)) => format!("{from} 至 {until}"),
                                (Some(from), None) => format!("{from} 起"),
                                (None, Some(until)) => format!("有效至 {until}"),
                                (None, None) => "未设置有效期".into(),
                            };
                            view! {
                                <article class=move || {
                                    let attached = attached_ids.get().contains(&source_id);
                                    let selected = selected_id.get() == Some(source_id);
                                    format!("knowledge-source{}{}{}", if attached { " is-attached" } else { "" }, if selected { " is-selected" } else { "" }, if archived { " is-archived" } else { "" })
                                }>
                                    <div class="knowledge-source-title">
                                        <span>{knowledge_type_label(&source.source_type)}</span>
                                        <strong>{source.name.clone()}</strong>
                                        <small class=if source.is_effective { "is-effective" } else { "" }>{knowledge_effective_label(&source)}</small>
                                    </div>
                                    <p>{source.description.clone()}</p>
                                    <div class="knowledge-source-meta">
                                        <code>{if source.version_label.is_empty() { "未标注版本".into() } else { format!("版本 {}", source.version_label) }}</code>
                                        <span>{validity}</span>
                                    </div>
                                    <div class="knowledge-source-actions">
                                        <label title="绑定后，当前有效内容会进入该项目的智能上下文">
                                            <input type="checkbox" prop:checked=move || attached_ids.get().contains(&source_id) on:change=move |event| toggle_attachment(source_id, event_target_checked(&event)) disabled=archived />
                                            <span>"绑定当前项目"</span>
                                        </label>
                                        <button class="button button-quiet" on:click=move |_| edit_source(source_for_edit.clone())>"编辑"</button>
                                        <button class="button button-quiet" on:click=move |_| archive_source(source_id) disabled=move || busy.get() || archived>"归档"</button>
                                    </div>
                                </article>
                            }
                        }
                    </For>
                    <Show when=move || {
                        let selected = filter.get();
                        let search = query.get().trim().to_lowercase();
                        !busy.get() && !sources.get().iter().any(|source| {
                            (selected == "all"
                                || (selected == "attached" && attached_ids.get().contains(&source.id))
                                || source.source_type == selected)
                                && (search.is_empty()
                                    || source.name.to_lowercase().contains(&search)
                                    || source.description.to_lowercase().contains(&search)
                                    || source.content.to_lowercase().contains(&search))
                        })
                    }>
                        <div class="knowledge-empty">"没有匹配的知识源"</div>
                    </Show>
                    </div>
                </div>
                <div class="knowledge-editor">
                    <div class="knowledge-editor-heading">
                        <div><strong>{move || if selected_id.get().is_some() { "编辑知识源" } else { "新建知识源" }}</strong><small>"保存后可按项目绑定，只有当前有效内容会进入智能上下文"</small></div>
                        <button class="button button-quiet" on:click=move |_| clear_form()>"清空"</button>
                    </div>
                    <div class="knowledge-form-grid">
                        <label><span>"类型"</span><select prop:value=source_type on:change=move |event| source_type.set(event_target_value(&event))><option value="term">"企业术语"</option><option value="field">"字段目录"</option><option value="policy">"制度政策"</option></select></label>
                        <label><span>"名称"</span><input maxlength="160" prop:value=name on:input=move |event| name.set(event_target_value(&event)) /></label>
                        <label><span>"版本"</span><input maxlength="80" placeholder="例如 2026.1" prop:value=version_label on:input=move |event| version_label.set(event_target_value(&event)) /></label>
                        <label class="knowledge-form-wide"><span>"说明"</span><input maxlength="4000" prop:value=description on:input=move |event| description.set(event_target_value(&event)) /></label>
                        <label><span>"生效日期"</span><input type="date" prop:value=valid_from on:input=move |event| valid_from.set(event_target_value(&event)) /></label>
                        <label><span>"失效日期"</span><input type="date" prop:value=valid_until on:input=move |event| valid_until.set(event_target_value(&event)) /></label>
                        <label class="knowledge-form-wide"><span>"来源地址"</span><input maxlength="2000" placeholder="制度页面、文档或数据字典地址" prop:value=source_uri on:input=move |event| source_uri.set(event_target_value(&event)) /></label>
                        <label class="knowledge-form-wide"><span>"知识内容"</span><textarea maxlength="50000" rows="7" prop:value=content on:input=move |event| content.set(event_target_value(&event))></textarea></label>
                    </div>
                    <button class="button button-primary" on:click=save_source disabled=move || busy.get() || name.get().trim().is_empty() || content.get().trim().is_empty()>{move || if busy.get() { "处理中..." } else if selected_id.get().is_some() { "保存修改" } else { "创建知识源" }}</button>
                    <Show when=move || error.get().is_some()><strong class="knowledge-error">{move || error.get().unwrap_or_default()}</strong></Show>
                </div>
            </div>
        </section>
    }
}

#[component]
pub fn DmnApp() -> impl IntoView {
    let token = auth_token();
    let (user_role, set_user_role) = signal(String::new());
    let (projects, set_projects) = signal(Vec::<DecisionProject>::new());
    let (current_project, set_current_project) = signal(Option::<DecisionProject>::None);
    let (current_version, set_current_version) = signal(Option::<DecisionVersion>::None);
    let (new_project_name, set_new_project_name) = signal(String::new());
    let (model_library, set_model_library) = signal(Vec::<ModelLibraryItem>::new());
    let (resource_view, set_resource_view) = signal(ResourceView::Knowledge);
    let (version_knowledge_citations, set_version_knowledge_citations) =
        signal(Vec::<KnowledgeCitation>::new());
    let (pending_knowledge_citations, set_pending_knowledge_citations) =
        signal(Vec::<KnowledgeCitation>::new());
    let (model_library_filter, set_model_library_filter) = signal("all".to_string());
    let (model_library_query, set_model_library_query) = signal(String::new());
    let (selected_model_source, set_selected_model_source) = signal(Option::<String>::None);
    let (reuse_project_name, set_reuse_project_name) = signal(String::new());
    let (reuse_confirmed, set_reuse_confirmed) = signal(false);
    let (template_name, set_template_name) = signal(String::new());
    let (template_industry, set_template_industry) = signal(String::new());
    let (model_library_busy, set_model_library_busy) = signal(false);
    let (model_library_error, set_model_library_error) = signal(Option::<String>::None);
    let (project_busy, set_project_busy) = signal(false);
    let (version_saving, set_version_saving) = signal(false);
    let (project_error, set_project_error) = signal(Option::<String>::None);
    let (version_save_status, set_version_save_status) = signal(Option::<String>::None);
    let (pending_review, set_pending_review) = signal(Option::<VersionChangeReview>::None);
    let (pending_candidate_id, set_pending_candidate_id) = signal(Option::<i64>::None);
    let (review_confirmed, set_review_confirmed) = signal(false);
    let (versions, set_versions) = signal(Vec::<DecisionVersion>::new());
    let (selected_version_id, set_selected_version_id) = signal(Option::<i64>::None);
    let (selected_version_review, set_selected_version_review) =
        signal(Option::<VersionChangeReview>::None);
    let (review_loading, set_review_loading) = signal(false);
    let (audit_events, set_audit_events) = signal(Vec::<AuditEvent>::new());
    let (project_members, set_project_members) = signal(Vec::<ProjectMember>::new());
    let (member_username, set_member_username) = signal(String::new());
    let (member_role, set_member_role) = signal("modeler".to_string());
    let (member_busy, set_member_busy) = signal(false);
    let (member_error, set_member_error) = signal(Option::<String>::None);
    let (member_status, set_member_status) = signal(Option::<String>::None);
    let (audit_revision, set_audit_revision) = signal(0_u64);
    let (rollback_reason, set_rollback_reason) = signal(String::new());
    let (rollback_confirmed, set_rollback_confirmed) = signal(false);
    let (rollback_busy, set_rollback_busy) = signal(false);
    let (verification_report, set_verification_report) =
        signal(Option::<VersionVerificationReport>::None);
    let (verification_busy, set_verification_busy) = signal(false);
    let (publish_confirmed, set_publish_confirmed) = signal(false);
    let (release_error, set_release_error) = signal(Option::<String>::None);
    let (decision_test_cases, set_decision_test_cases) = signal(Vec::<DecisionTestCase>::new());
    let (selected_test_case_ids, set_selected_test_case_ids) = signal(HashSet::<i64>::new());
    let (test_case_name, set_test_case_name) = signal(String::new());
    let (test_case_input, set_test_case_input) = signal("{}".to_string());
    let (test_case_expected, set_test_case_expected) = signal("{}".to_string());
    let (test_case_busy, set_test_case_busy) = signal(false);
    let (test_case_running, set_test_case_running) = signal(false);
    let (test_case_run_summary, set_test_case_run_summary) = signal(Option::<String>::None);
    let (test_case_load_revision, set_test_case_load_revision) = signal(0_u64);
    // Keep the list tied to the project/version it was loaded for. This lets
    // us clear stale cases on navigation while ignoring a transient empty
    // response that arrives after generation already populated the list.
    let (test_case_context, set_test_case_context) = signal(Option::<(i64, i64)>::None);
    let (test_case_error, set_test_case_error) = signal(Option::<String>::None);
    let (test_case_generation_summary, set_test_case_generation_summary) =
        signal(Option::<String>::None);
    let (published_services, set_published_services) =
        signal(Vec::<PublishedDecisionService>::new());
    let (service_keys, set_service_keys) = signal(Vec::<DecisionServiceApiKey>::new());
    let (service_key_name, set_service_key_name) = signal(String::new());
    let (service_key_secret, set_service_key_secret) = signal(Option::<String>::None);
    let (service_key_copy_status, set_service_key_copy_status) = signal(CopyStatus::Idle);
    let (service_key_service_id, set_service_key_service_id) = signal(Option::<String>::None);
    let (service_curl_copy_status, set_service_curl_copy_status) = signal(CopyStatus::Idle);
    let (service_endpoint_copied, set_service_endpoint_copied) = signal(Option::<String>::None);
    let (service_key_busy, set_service_key_busy) = signal(false);
    let (service_key_error, set_service_key_error) = signal(Option::<String>::None);
    let (selected_service_id, set_selected_service_id) = signal(Option::<String>::None);
    let (service_business_reference, set_service_business_reference) = signal(String::new());
    let (service_input, set_service_input) = signal("{}".to_string());
    let (service_run, set_service_run) = signal(Option::<DecisionServiceRun>::None);
    let (service_runs, set_service_runs) = signal(Vec::<DecisionServiceRun>::new());
    let (decision_analytics, set_decision_analytics) =
        signal(Option::<DecisionAnalyticsReport>::None);
    let (analytics_loading, set_analytics_loading) = signal(false);
    let (analytics_error, set_analytics_error) = signal(Option::<String>::None);
    let (analytics_revision, set_analytics_revision) = signal(0_u64);
    let (optimization_proposals, set_optimization_proposals) =
        signal(Vec::<DecisionOptimizationProposal>::new());
    let (proposal_busy, set_proposal_busy) = signal(false);
    let (proposal_error, set_proposal_error) = signal(Option::<String>::None);
    let (service_busy, set_service_busy) = signal(false);
    let (service_error, set_service_error) = signal(Option::<String>::None);
    let (outcome_run_id, set_outcome_run_id) = signal(Option::<i64>::None);
    let (outcome_business_reference, set_outcome_business_reference) = signal(String::new());
    let (outcome_actual_json, set_outcome_actual_json) = signal("{}".to_string());
    let (outcome_assessment, set_outcome_assessment) = signal("confirmed".to_string());
    let (outcome_note, set_outcome_note) = signal(String::new());
    let (outcome_occurred_at, set_outcome_occurred_at) = signal(String::new());
    let (outcome_busy, set_outcome_busy) = signal(false);
    let (decision_workflow, set_decision_workflow) = signal(Option::<DecisionWorkflowState>::None);
    let (workflow_busy, set_workflow_busy) = signal(false);
    let (workflow_analyzing, set_workflow_analyzing) = signal(false);
    let (workflow_error, set_workflow_error) = signal(Option::<String>::None);
    let (clarification_answers, set_clarification_answers) =
        signal(BTreeMap::<String, String>::new());
    let (prompt, set_prompt) = signal(String::new());
    let (messages, set_messages) = signal(Vec::<ChatMessage>::new());
    let (steps, set_steps) = signal(Vec::<String>::new());
    let (assistant_delta, set_assistant_delta) = signal(String::new());
    let (agent_activity, set_agent_activity) = signal("等待新的建模任务".to_string());
    let (dsl, set_dsl) = signal(Option::<String>::None);
    let (source_dsl, set_source_dsl) = signal(String::new());
    let (xml, set_xml) = signal(Option::<String>::None);
    let (error, set_error) = signal(Option::<String>::None);
    let (status, set_status) = signal(RunStatus::Idle);
    let (generation_mode, set_generation_mode) = signal(GenerationMode::Create);
    // This flag distinguishes an explicit plan-to-model action from ordinary
    // free-form model edits after a project already has a draft version.
    let (plan_generation_requested, set_plan_generation_requested) = signal(false);
    let (controller, set_controller) = signal(Option::<AbortController>::None);
    let (theme, set_theme) = signal(Theme::Light);
    // 在工作流尚未加载前展示需求分析；加载后根据是否存在方案切换到对应 Tab。
    let (workspace_view, set_workspace_view) = signal(WorkspaceView::Discovery);
    let (show_test, set_show_test) = signal(false);
    let (test_xml, set_test_xml) = signal(String::new());
    let (test_input, set_test_input) = signal("{}".to_string());
    let (analysis_running, set_analysis_running) = signal(false);
    let (test_result, set_test_result) = signal(Option::<EvaluateResult>::None);
    let (test_error, set_test_error) = signal(Option::<String>::None);
    let (test_running, set_test_running) = signal(false);
    let (validation_result, set_validation_result) = signal(Option::<api::ValidateDmnResult>::None);
    let (validation_error, set_validation_error) = signal(Option::<String>::None);
    let (validation_running, set_validation_running) = signal(false);
    let message_list = NodeRef::<leptos::html::Div>::new();
    let test_drawer_body = NodeRef::<leptos::html::Div>::new();

    Effect::new(move |_| {
        let Some(token_value) = token.get() else {
            set_user_role.set(String::new());
            return;
        };
        spawn_local(async move {
            if let Some((_, role)) = api::me(&token_value).await {
                set_user_role.set(role);
            }
        });
    });

    Effect::new(move |_| {
        let Some(token_value) = token.get() else {
            return;
        };
        set_project_busy.set(true);
        spawn_local(async move {
            match api::list_projects(&token_value).await {
                Ok(items) => {
                    set_projects.set(items.clone());
                    if let Some(project) = items.first().cloned() {
                        match api::get_project_workspace(&token_value, project.id).await {
                            Ok(workspace) => {
                                let has_version = workspace.version.is_some();
                                let version = workspace.version.clone();
                                set_current_project.set(Some(workspace.project));
                                set_current_version.set(version.clone());
                                set_pending_review.set(None);
                                set_review_confirmed.set(false);
                                set_selected_version_id.set(None);
                                set_selected_version_review.set(None);
                                set_review_loading.set(false);
                                set_verification_report.set(None);
                                set_publish_confirmed.set(false);
                                set_release_error.set(None);
                                set_dsl.set(workspace.dsl_yaml.clone());
                                set_source_dsl.set(workspace.dsl_yaml.unwrap_or_default());
                                set_xml.set(workspace.dmn_xml.clone());
                                set_test_xml.set(workspace.dmn_xml.unwrap_or_default());
                                set_test_result.set(None);
                                set_test_error.set(None);
                                set_validation_result.set(None);
                                set_validation_error.set(None);
                                set_steps.set(Vec::new());
                                set_assistant_delta.set(String::new());
                                set_agent_activity.set("等待新的建模任务".into());
                                set_status.set(RunStatus::Idle);
                                set_error.set(None);
                                set_version_knowledge_citations.set(workspace.knowledge_citations);
                                set_generation_mode.set(if has_version {
                                    GenerationMode::Modify
                                } else {
                                    GenerationMode::Create
                                });
                                match api::list_project_versions(&token_value, project.id).await {
                                    Ok(items) => set_versions.set(items),
                                    Err(message) => set_project_error.set(Some(message)),
                                }
                                match api::list_project_audit_events(&token_value, project.id).await
                                {
                                    Ok(events) => set_audit_events.set(events),
                                    Err(message) => set_project_error.set(Some(message)),
                                }
                                if let Some(version) = version {
                                    match api::get_version_verification(
                                        &token_value,
                                        project.id,
                                        version.id,
                                    )
                                    .await
                                    {
                                        Ok(report) => set_verification_report.set(report),
                                        Err(message) => set_release_error.set(Some(message)),
                                    }
                                }
                                match api::get_decision_workflow(&token_value, project.id).await {
                                    Ok(workflow) => {
                                        let answers = workflow
                                            .as_ref()
                                            .map(|state| {
                                                state
                                                    .answers
                                                    .iter()
                                                    .map(|answer| {
                                                        (
                                                            answer.clarification_id.clone(),
                                                            answer.answer.clone(),
                                                        )
                                                    })
                                                    .collect()
                                            })
                                            .unwrap_or_default();
                                        set_clarification_answers.set(answers);
                                        set_prompt.set(
                                            workflow
                                                .as_ref()
                                                .map(|state| state.requirement_text.clone())
                                                .unwrap_or_default(),
                                        );
                                        // 新增模型必须从需求与方案页的确认方案入口触发。
                                        set_workspace_view.set(WorkspaceView::Discovery);
                                        set_decision_workflow.set(workflow);
                                    }
                                    Err(message) => set_workflow_error.set(Some(message)),
                                }
                            }
                            Err(message) => set_project_error.set(Some(message)),
                        }
                    }
                }
                Err(message) => set_project_error.set(Some(message)),
            }
            set_project_busy.set(false);
        });
    });

    Effect::new(move |_| {
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            set_project_members.set(Vec::new());
            set_member_status.set(None);
            return;
        };
        set_member_status.set(None);
        spawn_local(async move {
            match api::list_project_members(&token_value, project.id).await {
                Ok(members)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_project_members.set(members);
                    set_member_error.set(None);
                    set_member_status.set(None);
                }
                Ok(_) => {}
                Err(message)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_member_error.set(Some(message));
                }
                Err(_) => {}
            }
        });
    });

    Effect::new(move |_| {
        let _ = audit_revision.get();
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            return;
        };
        spawn_local(async move {
            if let Ok(events) = api::list_project_audit_events(&token_value, project.id).await
                && current_project
                    .get_untracked()
                    .is_some_and(|current| current.id == project.id)
            {
                set_audit_events.set(events);
            }
        });
    });

    Effect::new(move |_| {
        let Some(token_value) = token.get() else {
            set_published_services.set(Vec::new());
            set_service_keys.set(Vec::new());
            return;
        };
        let Some(project) = current_project.get() else {
            set_published_services.set(Vec::new());
            set_service_keys.set(Vec::new());
            return;
        };
        if project.published_version_id.is_none() {
            set_published_services.set(Vec::new());
            set_service_keys.set(Vec::new());
            set_selected_service_id.set(None);
            set_service_run.set(None);
            set_service_runs.set(Vec::new());
            set_service_business_reference.set(String::new());
            set_outcome_run_id.set(None);
            return;
        }
        set_service_error.set(None);
        set_service_key_error.set(None);
        spawn_local(async move {
            let runs = api::list_decision_service_runs(&token_value, project.id).await;
            let keys = api::list_decision_service_api_keys(&token_value, project.id).await;
            match api::list_published_decision_services(&token_value, project.id).await {
                Ok(services)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    let selected = services.first().cloned();
                    set_selected_service_id
                        .set(selected.as_ref().map(|service| service.id.clone()));
                    set_service_input.set(
                        selected
                            .as_ref()
                            .map(service_input_template)
                            .unwrap_or_else(|| "{}".into()),
                    );
                    set_service_run.set(None);
                    set_service_business_reference.set(String::new());
                    set_outcome_run_id.set(None);
                    set_published_services.set(services);
                    match keys {
                        Ok(keys) => set_service_keys.set(keys),
                        Err(message) => set_service_key_error.set(Some(message)),
                    }
                    match runs {
                        Ok(runs) => set_service_runs.set(runs),
                        Err(message) => set_service_error.set(Some(message)),
                    }
                }
                Ok(_) => {}
                Err(message) => set_service_error.set(Some(message)),
            }
        });
    });

    Effect::new(move |_| {
        let _ = analytics_revision.get();
        let Some(token_value) = token.get() else {
            set_decision_analytics.set(None);
            set_analytics_loading.set(false);
            set_analytics_error.set(None);
            return;
        };
        let Some(project) = current_project.get() else {
            set_decision_analytics.set(None);
            set_analytics_loading.set(false);
            set_analytics_error.set(None);
            return;
        };
        if project.published_version_id.is_none() {
            set_decision_analytics.set(None);
            set_analytics_loading.set(false);
            set_analytics_error.set(None);
            return;
        }
        if decision_analytics.get_untracked().is_some_and(|report| {
            report.project_id != project.id
                || Some(report.version_id) != project.published_version_id
        }) {
            set_decision_analytics.set(None);
        }
        set_analytics_loading.set(true);
        set_analytics_error.set(None);
        spawn_local(async move {
            match api::get_decision_analytics(&token_value, project.id).await {
                Ok(report)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_decision_analytics.set(Some(report));
                }
                Ok(_) => {}
                Err(message)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_analytics_error.set(Some(message));
                }
                Err(_) => {}
            }
            if current_project
                .get_untracked()
                .is_some_and(|current| current.id == project.id)
            {
                set_analytics_loading.set(false);
            }
        });
    });

    Effect::new(move |_| {
        let _ = analytics_revision.get();
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            set_optimization_proposals.set(Vec::new());
            set_proposal_error.set(None);
            return;
        };
        if project.published_version_id.is_none() {
            set_optimization_proposals.set(Vec::new());
            set_proposal_error.set(None);
            return;
        }
        spawn_local(async move {
            match api::list_optimization_proposals(&token_value, project.id).await {
                Ok(proposals)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_optimization_proposals.set(proposals);
                }
                Ok(_) => {}
                Err(message)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_proposal_error.set(Some(message));
                }
                Err(_) => {}
            }
        });
    });

    Effect::new(move |_| {
        let _ = messages.get();
        let _ = assistant_delta.get();
        // 生成完成后直接定位到变更确认区，避免保存入口落在可滚动区域外。
        let _ = pending_review.get();
        if let Some(element) = message_list.get() {
            element.set_scroll_top(element.scroll_height());
        }
    });

    Effect::new(move |_| {
        let has_result = test_result.get().is_some() || test_error.get().is_some();
        if has_result && let Some(element) = test_drawer_body.get() {
            element.set_scroll_top(element.scroll_height());
        }
    });

    Effect::new(move |_| {
        let Some(token_value) = token.get() else {
            set_decision_test_cases.set(Vec::new());
            set_selected_test_case_ids.set(HashSet::new());
            set_test_case_context.set(None);
            set_test_case_generation_summary.set(None);
            set_test_case_run_summary.set(None);
            return;
        };
        let (Some(project), Some(version)) = (current_project.get(), current_version.get()) else {
            set_decision_test_cases.set(Vec::new());
            set_selected_test_case_ids.set(HashSet::new());
            set_test_case_context.set(None);
            set_test_case_generation_summary.set(None);
            set_test_case_run_summary.set(None);
            return;
        };
        let context = (project.id, version.id);
        // Clear cases synchronously when the active project/version changes so
        // a previous suite is never shown while the new suite is loading.
        if test_case_context.get_untracked() != Some(context) {
            set_test_case_context.set(Some(context));
            set_decision_test_cases.set(Vec::new());
            set_selected_test_case_ids.set(HashSet::new());
        }
        let request_revision = test_case_load_revision.get();
        set_test_case_error.set(None);
        set_test_case_generation_summary.set(None);
        set_test_case_run_summary.set(None);
        spawn_local(async move {
            match api::list_decision_test_cases(&token_value, project.id, version.id).await {
                Ok(cases)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id)
                        && current_version
                            .get_untracked()
                            .is_some_and(|current| current.id == version.id)
                        && test_case_context.get_untracked() == Some(context)
                        && test_case_load_revision.get_untracked() == request_revision =>
                {
                    // A generation request can commit before a concurrent
                    // list request observes the new rows. Do not let that
                    // transient empty response erase cases already rendered
                    // for the same project/version. Navigation clears the
                    // signal synchronously before starting a new request, so
                    // an empty suite still loads normally there.
                    if cases.is_empty() && !decision_test_cases.get_untracked().is_empty() {
                        return;
                    }
                    set_decision_test_cases.set(cases);
                }
                Ok(_) => {}
                Err(message)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id)
                        && current_version
                            .get_untracked()
                            .is_some_and(|current| current.id == version.id)
                        && test_case_context.get_untracked() == Some(context)
                        && test_case_load_revision.get_untracked() == request_revision =>
                {
                    set_test_case_error.set(Some(message));
                }
                Err(_) => {}
            }
        });
    });

    let generate_optimization_proposals = move |_| {
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            return;
        };
        if proposal_busy.get() {
            return;
        }
        set_proposal_busy.set(true);
        set_proposal_error.set(None);
        spawn_local(async move {
            match api::generate_optimization_proposals(&token_value, project.id).await {
                Ok(proposals) => {
                    set_optimization_proposals.set(proposals);
                    set_version_save_status.set(Some(
                        "已基于当前发布版本生成可追溯优化建议，需人工确认后才会创建草稿".into(),
                    ));
                    set_analytics_revision.update(|revision| *revision += 1);
                    set_audit_revision.update(|revision| *revision += 1);
                }
                Err(message) => set_proposal_error.set(Some(message)),
            }
            set_proposal_busy.set(false);
        });
    };

    let dismiss_optimization_proposal = move |proposal_id: i64| {
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            return;
        };
        if proposal_busy.get() {
            return;
        }
        set_proposal_busy.set(true);
        set_proposal_error.set(None);
        spawn_local(async move {
            match api::dismiss_optimization_proposal(&token_value, project.id, proposal_id).await {
                Ok(updated) => {
                    set_optimization_proposals.update(|proposals| {
                        if let Some(proposal) =
                            proposals.iter_mut().find(|item| item.id == proposal_id)
                        {
                            *proposal = updated;
                        }
                    });
                    set_version_save_status.set(Some("优化建议已驳回并保留审计记录".into()));
                    set_audit_revision.update(|revision| *revision += 1);
                }
                Err(message) => set_proposal_error.set(Some(message)),
            }
            set_proposal_busy.set(false);
        });
    };

    let confirm_optimization_proposal = move |proposal_id: i64| {
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            return;
        };
        if proposal_busy.get() {
            return;
        }
        set_proposal_busy.set(true);
        set_proposal_error.set(None);
        spawn_local(async move {
            match api::confirm_optimization_proposal(&token_value, project.id, proposal_id).await {
                Ok(workspace) => {
                    let version = workspace.version.clone();
                    set_current_project.set(Some(workspace.project));
                    set_current_version.set(version.clone());
                    set_dsl.set(workspace.dsl_yaml.clone());
                    set_source_dsl.set(workspace.dsl_yaml.unwrap_or_default());
                    set_xml.set(workspace.dmn_xml);
                    set_version_knowledge_citations.set(workspace.knowledge_citations);
                    set_generation_mode.set(GenerationMode::Modify);
                    set_pending_review.set(None);
                    set_review_confirmed.set(false);
                    set_verification_report.set(None);
                    set_publish_confirmed.set(false);
                    set_workspace_view.set(WorkspaceView::Model);
                    set_test_case_load_revision.update(|revision| *revision += 1);
                    match api::list_project_versions(&token_value, project.id).await {
                        Ok(versions) => set_versions.set(versions),
                        Err(message) => set_project_error.set(Some(message)),
                    }
                    set_version_save_status.set(Some(
                        "优化草稿已创建：请在建模页根据建议修改模型，再完成验收与发布".into(),
                    ));
                    set_analytics_revision.update(|revision| *revision += 1);
                    set_audit_revision.update(|revision| *revision += 1);
                }
                Err(message) => set_proposal_error.set(Some(message)),
            }
            set_proposal_busy.set(false);
        });
    };

    let submit = move || {
        let mode = generation_mode.get();
        let plan_generation = plan_generation_requested.get();
        let prompt_text = prompt.get();
        let user_text = if prompt_text.trim().is_empty() && mode == GenerationMode::Create {
            decision_workflow
                .get()
                .map(|workflow| workflow.requirement_text.trim().to_string())
                .unwrap_or_default()
        } else {
            prompt_text.trim().to_string()
        };
        if user_text.is_empty() || status.get() == RunStatus::Running {
            return;
        }
        let Some(token_value) = token.get() else {
            return;
        };
        let Some(project_id) = current_project.get().map(|project| project.id) else {
            set_error.set(Some("请先创建或选择一个决策项目".to_string()));
            return;
        };
        if mode == GenerationMode::Modify {
            let Some(base) = current_version.get() else {
                set_error.set(Some("当前没有可修改的草稿版本".into()));
                return;
            };
            if source_dsl.get().trim().is_empty() {
                set_error.set(Some("请先加载当前 DSL 后再修改".into()));
                return;
            }
            set_status.set(RunStatus::Running);
            set_error.set(None);
            set_pending_review.set(None);
            set_pending_candidate_id.set(None);
            set_dsl.set(None);
            set_xml.set(None);
            set_test_xml.set(String::new());
            set_test_input.set("{}".to_string());
            set_test_result.set(None);
            set_test_error.set(None);
            set_validation_result.set(None);
            set_validation_error.set(None);
            set_steps.set(vec!["→ 正在生成模型变更意图...".into()]);
            set_agent_activity.set("正在生成模型变更意图".into());
            set_assistant_delta.set(String::new());
            set_version_save_status.set(Some("正在生成模型变更意图...".into()));
            let controller_value =
                AbortController::new().expect("AbortController should be available");
            set_controller.set(Some(controller_value.clone()));
            let set_dsl = set_dsl;
            let set_xml = set_xml;
            let set_test_xml = set_test_xml;
            let set_test_result = set_test_result;
            let set_test_error = set_test_error;
            let set_validation_result = set_validation_result;
            let set_validation_error = set_validation_error;
            let set_steps = set_steps;
            let set_agent_activity = set_agent_activity;
            let set_messages = set_messages;
            let set_workspace_view = set_workspace_view;
            let set_controller = set_controller;
            let set_status = set_status;
            spawn_local(async move {
                let change_request = if plan_generation {
                    decision_workflow
                        .get_untracked()
                        .filter(|workflow| workflow.status == "confirmed")
                        .and_then(|workflow| workflow.plan.map(|plan| (workflow.brief, plan)))
                        .map(|(brief, plan)| {
                            format!(
                                "请将已确认的决策方案落实为对当前 DMN 模型的最小变更意图。\n\n已确认决策方案：{}\n\n需求摘要：{}",
                                serde_json::to_string_pretty(&plan).unwrap_or_default(),
                                serde_json::to_string_pretty(&brief).unwrap_or_default()
                            )
                        })
                        .unwrap_or_else(|| user_text.clone())
                } else {
                    user_text.clone()
                };
                let mut repair_attempt = 0;
                let mut repair_error = None;
                let mut previous_intent: Option<ModelChangeIntent> = None;
                let result = loop {
                    let mut generated_intent = None;
                    let intent_stream_result = api::generate_change_intent_stream(
                        &token_value,
                        project_id,
                        change_request.clone(),
                        base.id,
                        repair_error.clone(),
                        repair_attempt,
                        previous_intent.clone(),
                        controller_value.clone(),
                        |event| match event {
                            SseEvent::Step(message) => {
                                set_agent_activity.set(agent_activity_for_step(&message));
                                set_steps.update(|items| items.push(message));
                            }
                            SseEvent::ChangeIntent(intent) => generated_intent = Some(intent),
                            SseEvent::Error(message) => {
                                set_steps.update(|items| {
                                    items.push(format!("⚠ 变更意图生成失败：{message}"))
                                });
                                set_agent_activity.set("变更意图生成失败".into());
                            }
                            SseEvent::Delta(_)
                            | SseEvent::Preview(_)
                            | SseEvent::Result(_)
                            | SseEvent::WorkflowResult(_) => {}
                        },
                    )
                    .await;
                    if controller_value.signal().aborted() {
                        set_controller.set(None);
                        set_status.set(RunStatus::Aborted);
                        return;
                    }
                    if let Err(error) = intent_stream_result {
                        break Err(error);
                    }
                    let intent = match generated_intent {
                        Some(intent) => intent,
                        None => break Err("变更意图流未返回有效结果".into()),
                    };
                    set_steps
                        .update(|items| items.push("✓ 变更意图生成完成，正在校验 DSL...".into()));
                    set_agent_activity.set("正在校验模型变更意图".into());
                    match api::preview_model_change(&token_value, project_id, intent.clone()).await
                    {
                        Ok(candidate) => break Ok(candidate),
                        Err(error)
                            if is_repairable_model_change_error(&error)
                                && repair_attempt < MAX_DMN_COMPILE_REPAIRS =>
                        {
                            repair_attempt += 1;
                            set_steps.update(|items| {
                                items.push(format!(
                                    "⚠ DMN 编译校验未通过，正在将错误反馈给模型修复（{repair_attempt}/{MAX_DMN_COMPILE_REPAIRS}）..."
                                ));
                            });
                            set_agent_activity.set(format!(
                                "正在根据 DMN 编译错误修复模型（{repair_attempt}/{MAX_DMN_COMPILE_REPAIRS}）"
                            ));
                            repair_error = Some(error);
                            previous_intent = Some(intent);
                        }
                        Err(error) => break Err(error),
                    }
                };
                match result {
                    Ok(candidate) => {
                        let dsl_yaml = candidate.dsl_yaml;
                        let dmn_xml = candidate.dmn_xml;
                        set_steps.update(|items| {
                            items.push("✓ DSL 校验通过，正在编译 DMN XML...".into());
                            items.push("✓ DMN XML 编译完成".into());
                        });
                        set_agent_activity.set("模型已生成，可继续核验或测试".into());
                        set_dsl.set(Some(dsl_yaml.clone()));
                        set_xml.set(Some(dmn_xml.clone()));
                        set_test_xml.set(dmn_xml);
                        set_test_result.set(None);
                        set_test_error.set(None);
                        set_validation_result.set(None);
                        set_validation_error.set(None);
                        set_workspace_view.set(WorkspaceView::Model);
                        set_messages.update(|items| {
                            items.push(ChatMessage {
                                role: "assistant",
                                content: "模型变更预览已生成并通过校验，等待确认后提交版本。"
                                    .into(),
                            });
                        });
                        set_pending_candidate_id.set(Some(candidate.candidate_id));
                        set_pending_review.set(candidate.change_review);
                        set_status.set(RunStatus::Succeeded);
                        set_version_save_status
                            .set(Some("模型变更预览已生成，请确认后提交版本".into()));
                    }
                    Err(message) => {
                        set_steps.update(|items| items.push(format!("⚠ 模型变更失败：{message}")));
                        set_agent_activity.set("生成失败，请修正需求后重试".into());
                        set_status.set(RunStatus::Failed);
                        set_error.set(Some(message));
                        set_version_save_status.set(Some("模型变更预览失败".into()));
                    }
                }
                set_controller.set(None);
            });
            return;
        }
        if mode == GenerationMode::Create
            && !decision_workflow
                .get()
                .as_ref()
                .is_some_and(|workflow| workflow.status == "confirmed")
        {
            set_error.set(Some("请先完成需求分析并确认决策方案".to_string()));
            return;
        }
        let text = if mode == GenerationMode::Create || plan_generation {
            decision_workflow
                .get()
                .filter(|workflow| workflow.status == "confirmed")
                .and_then(|workflow| workflow.plan.map(|plan| (workflow.brief, plan)))
                .map(|(brief, plan)| {
                    format!(
                        "## 生成任务\n请严格将下面的已确认决策方案转换为可执行 DMN DSL。已确认方案是唯一的结构和规则依据，不要重新分析需求、不要自行增加或删除决策、输入、依赖、知识模型或服务。\n\n## 已确认决策方案（权威输入，必须完整落实）\n{}\n\n## 需求摘要（仅用于理解业务语义，不得覆盖方案）\n{}\n\n## 生成约束\n- 保留方案中的决策名称、输入输出、依赖关系和验收标准。\n- 对方案中未明确的内容使用最小化实现，不凭空扩展业务规则。\n- 生成完整、可校验的 DMN DSL。",
                        serde_json::to_string_pretty(&plan).unwrap_or_default(),
                        serde_json::to_string_pretty(&brief).unwrap_or_default()
                    )
                })
                .unwrap_or_else(|| user_text.clone())
        } else {
            user_text.clone()
        };
        let base_version = current_version.get().map(|version| version.version_number);
        let existing = match mode {
            GenerationMode::Create => None,
            GenerationMode::Modify => {
                let current_dsl = source_dsl.get();
                if current_dsl.trim().is_empty() {
                    set_error.set(Some("请先粘贴需要修改的 DSL YAML".to_string()));
                    return;
                }
                Some(current_dsl)
            }
        };
        let request_for_retry = if plan_generation {
            text.clone()
        } else {
            user_text.clone()
        };
        // Keep the requested change visible while it runs and after the draft
        // is saved, so the next incremental update can be submitted directly.
        set_status.set(RunStatus::Running);
        // Invalidate the previous artifact snapshot immediately. Keeping old
        // DSL/XML visible while the LLM is running makes it easy to mistake a
        // previous model for the result of the current request.
        set_dsl.set(None);
        set_xml.set(None);
        set_test_xml.set(String::new());
        set_test_input.set("{}".to_string());
        set_test_result.set(None);
        set_test_error.set(None);
        set_test_running.set(false);
        set_validation_result.set(None);
        set_validation_error.set(None);
        set_validation_running.set(false);
        set_analysis_running.set(false);
        set_error.set(None);
        set_pending_review.set(None);
        set_pending_candidate_id.set(None);
        set_pending_knowledge_citations.set(Vec::new());
        set_review_confirmed.set(false);
        set_steps.set(vec![match mode {
            GenerationMode::Create => "→ 正在准备新模型...".to_string(),
            GenerationMode::Modify => "→ 正在准备当前 DSL 的增量修改...".to_string(),
        }]);
        set_assistant_delta.set(String::new());
        set_agent_activity.set(match mode {
            GenerationMode::Create => "正在准备新模型请求".to_string(),
            GenerationMode::Modify => "正在准备增量修改请求".to_string(),
        });
        set_messages.update(|items| {
            items.push(ChatMessage {
                role: "user",
                content: user_text,
            });
        });

        let controller_value = AbortController::new().expect("AbortController should be available");
        set_controller.set(Some(controller_value.clone()));
        let set_steps = set_steps;
        let set_assistant_delta = set_assistant_delta;
        let set_agent_activity = set_agent_activity;
        let set_dsl = set_dsl;
        let set_xml = set_xml;
        let set_error = set_error;
        let set_status = set_status;
        let set_controller = set_controller;
        let set_messages = set_messages;
        let set_prompt = set_prompt;
        let set_generation_mode = set_generation_mode;
        let set_source_dsl = set_source_dsl;
        let set_version_save_status = set_version_save_status;
        let set_plan_generation_requested = set_plan_generation_requested;
        spawn_local(async move {
            let result = api::run_pipeline(
                &token_value,
                project_id,
                text,
                existing,
                base_version,
                controller_value.clone(),
                |event| match event {
                    SseEvent::Step(message) => {
                        if message.starts_with("→ 调用 LLM") {
                            set_assistant_delta.set(String::new());
                        }
                        set_agent_activity.set(agent_activity_for_step(&message));
                        set_steps.update(|items| items.push(message));
                    }
                    SseEvent::Delta(content) => {
                        if !content.is_empty() {
                            set_agent_activity.set("正在接收结构化输出，等待校验".to_string());
                        }
                    }
                    SseEvent::Preview(content) => {
                        let is_first_preview = assistant_delta.get_untracked().is_empty();
                        set_assistant_delta.update(|value| value.push_str(&content));
                        if is_first_preview && !content.is_empty() {
                            set_agent_activity.set("正在呈现已验证的 DSL".to_string());
                            set_steps.update(|items| {
                                items.push("✓ 已通过校验，正在流式呈现 DSL".to_string())
                            });
                        }
                    }
                    SseEvent::Result(result) => {
                        set_plan_generation_requested.set(false);
                        if result.ok {
                            set_pending_knowledge_citations.set(result.knowledge_citations.clone());
                            set_agent_activity.set("模型已生成，可继续核验或测试".to_string());
                            set_steps.update(|items| items.push("✓ DMN XML 生成完成".to_string()));
                            if result.dsl.is_some() && result.xml.is_some() {
                                set_pending_review.set(result.change_review.clone());
                                set_review_confirmed.set(false);
                                set_version_save_status
                                    .set(Some("模型已生成，等待确认变更后保存版本".into()));
                            }
                        }
                        apply_result(
                            result,
                            set_dsl,
                            set_xml,
                            set_test_xml,
                            set_test_result,
                            set_test_error,
                            set_validation_result,
                            set_validation_error,
                            set_error,
                            set_messages,
                            set_status,
                            set_generation_mode,
                            set_source_dsl,
                            set_workspace_view,
                        );
                    }
                    SseEvent::Error(message) => {
                        set_plan_generation_requested.set(false);
                        set_assistant_delta.set(String::new());
                        set_prompt.set(request_for_retry.clone());
                        set_agent_activity.set("生成失败，请修正需求后重试".to_string());
                        set_error.set(Some(message));
                        set_status.set(RunStatus::Failed);
                    }
                    SseEvent::WorkflowResult(_) => {}
                    SseEvent::ChangeIntent(_) => {}
                },
            )
            .await;

            if controller_value.signal().aborted() {
                set_controller.set(None);
                set_status.set(RunStatus::Aborted);
                set_agent_activity.set("已停止当前生成任务".to_string());
                return;
            }
            if let Err(message) = result {
                set_plan_generation_requested.set(false);
                set_assistant_delta.set(String::new());
                set_prompt.set(request_for_retry);
                set_agent_activity.set("请求未完成，请检查错误信息后重试".to_string());
                set_error.set(Some(message));
                set_status.set(RunStatus::Failed);
            }
            set_controller.set(None);
        });
    };

    let confirm_change = move |_| {
        let Some(project) = current_project.get() else {
            return;
        };
        let Some(token_value) = token.get() else {
            return;
        };
        let Some(dsl_yaml) = dsl.get() else {
            set_error.set(Some("没有可保存的 DSL 结果".into()));
            return;
        };
        let Some(dmn_xml) = xml.get() else {
            set_error.set(Some("没有可保存的 DMN XML 结果".into()));
            return;
        };
        if version_saving.get() || pending_review.get().is_none() {
            return;
        }
        let workflow_snapshot = decision_workflow.get();
        let change_summary = pending_review.get().map(|review| {
            serde_json::json!({
                "added": review.changes.iter().filter(|change| change.change_type == "add").map(|change| change.element.clone()).collect::<Vec<_>>(),
                "updated": review.changes.iter().filter(|change| change.change_type == "update").map(|change| change.element.clone()).collect::<Vec<_>>(),
                "removed": review.changes.iter().filter(|change| change.change_type == "remove").map(|change| change.element.clone()).collect::<Vec<_>>(),
            })
        });
        let submitted_dsl = dsl_yaml.clone();
        let submitted_xml = dmn_xml.clone();
        set_version_saving.set(true);
        set_status.set(RunStatus::Running);
        set_agent_activity.set("正在提交已确认的模型变更".into());
        set_steps.update(|items| items.push("→ 正在提交已确认的模型变更...".into()));
        set_version_save_status.set(Some("正在提交已确认的模型变更...".into()));
        spawn_local(async move {
            let result = if let Some(candidate_id) = pending_candidate_id.get_untracked() {
                api::commit_model_change(&token_value, project.id, candidate_id).await
            } else {
                api::create_project_version(
                    &token_value,
                    project.id,
                    CreateDecisionVersion {
                        dsl_yaml,
                        dmn_xml,
                        change_summary,
                        decision_brief: workflow_snapshot
                            .as_ref()
                            .and_then(|workflow| serde_json::to_value(&workflow.brief).ok()),
                        decision_plan: workflow_snapshot
                            .and_then(|workflow| workflow.plan)
                            .and_then(|plan| serde_json::to_value(plan).ok()),
                        candidate_id: None,
                        knowledge_citations: pending_knowledge_citations.get(),
                        change_confirmed: true,
                    },
                )
                .await
            };
            match result {
                Ok(version) => {
                    let version_number = version.version_number;
                    // The submitted candidate has already passed DSL and DMN
                    // compilation. Show it immediately as a reliable fallback,
                    // then replace it with the authoritative persisted snapshot.
                    set_current_version.set(Some(version.clone()));
                    set_version_knowledge_citations.set(pending_knowledge_citations.get());
                    set_versions.update(|items| {
                        items.retain(|item| item.id != version.id);
                        items.insert(0, version);
                    });
                    set_dsl.set(Some(submitted_dsl.clone()));
                    set_source_dsl.set(submitted_dsl.clone());
                    set_xml.set(Some(submitted_xml.clone()));
                    set_test_xml.set(submitted_xml.clone());
                    set_test_result.set(None);
                    set_test_error.set(None);
                    set_validation_result.set(None);
                    set_validation_error.set(None);
                    set_pending_review.set(None);
                    set_pending_candidate_id.set(None);
                    set_review_confirmed.set(true);
                    set_verification_report.set(None);
                    set_publish_confirmed.set(false);
                    set_release_error.set(None);
                    set_steps.update(|items| {
                        items.push(format!("✓ 草稿 v{version_number} 已提交"));
                        items.push("→ 正在刷新模型产物和版本信息...".into());
                    });
                    set_agent_activity.set("正在刷新模型产物和版本信息".into());
                    set_version_save_status.set(Some(format!(
                        "草稿 v{version_number} 已提交，正在刷新模型产物..."
                    )));
                    // The commit endpoint returns the version metadata. Refresh
                    // the workspace as the authoritative source for artifacts
                    // and the current draft pointer before accepting another edit.
                    match api::get_project_workspace(&token_value, project.id).await {
                        Ok(workspace) => {
                            set_current_project.set(Some(workspace.project));
                            set_current_version.set(workspace.version);
                            set_dsl.set(workspace.dsl_yaml.clone());
                            set_source_dsl.set(workspace.dsl_yaml.unwrap_or_default());
                            set_xml.set(workspace.dmn_xml.clone());
                            set_test_xml.set(workspace.dmn_xml.unwrap_or_default());
                            set_version_knowledge_citations.set(workspace.knowledge_citations);
                            set_steps
                                .update(|items| items.push("✓ 模型产物已从当前草稿刷新".into()));
                            set_version_save_status.set(Some(format!(
                                "草稿 v{version_number} 已保存，模型产物已刷新"
                            )));
                        }
                        Err(message) => {
                            set_steps.update(|items| {
                                items.push(format!(
                                    "⚠ 草稿已保存，但权威模型产物回读失败：{message}"
                                ))
                            });
                            set_version_save_status.set(Some(format!(
                                "草稿 v{version_number} 已保存；当前显示已校验候选，但服务端产物刷新失败：{message}"
                            )));
                        }
                    }
                    match api::list_project_versions(&token_value, project.id).await {
                        Ok(items) => set_versions.set(items),
                        Err(message) => set_project_error.set(Some(message)),
                    }
                    set_test_case_load_revision.update(|revision| *revision += 1);
                    set_audit_revision.update(|revision| *revision += 1);
                    set_agent_activity.set("模型变更已提交，等待新的建模任务".into());
                    set_steps.update(|items| {
                        items.push(format!("✓ 草稿 v{version_number} 已就绪，可继续核验或测试"));
                    });
                    set_messages.update(|items| {
                        items.push(ChatMessage {
                            role: "assistant",
                            content: format!(
                                "模型变更已提交为草稿 v{version_number}，模型产物和测试上下文已刷新。"
                            ),
                        });
                    });
                    set_status.set(RunStatus::Idle);
                }
                Err(message) => {
                    set_agent_activity.set("模型变更提交失败，请检查错误后重试".into());
                    set_status.set(RunStatus::Failed);
                    if message.starts_with("[BASE_CONFLICT]") {
                        set_version_save_status.set(Some(
                            "当前草稿已被其他修改更新，请刷新项目后重新生成。".into(),
                        ));
                    } else {
                        set_version_save_status.set(Some(format!("版本保存失败：{message}")));
                    }
                    set_steps.update(|items| items.push(format!("⚠ 模型变更提交失败：{message}")));
                }
            }
            set_version_saving.set(false);
        });
    };

    let discard_change = move |_| {
        let Some(project) = current_project.get() else {
            return;
        };
        let Some(token_value) = token.get() else {
            return;
        };
        if version_saving.get() {
            return;
        }
        set_project_busy.set(true);
        set_version_save_status.set(Some("正在恢复当前草稿...".into()));
        spawn_local(async move {
            match api::get_project_workspace(&token_value, project.id).await {
                Ok(workspace) => {
                    let has_version = workspace.version.is_some();
                    set_current_version.set(workspace.version);
                    set_dsl.set(workspace.dsl_yaml.clone());
                    set_source_dsl.set(workspace.dsl_yaml.unwrap_or_default());
                    set_xml.set(workspace.dmn_xml.clone());
                    set_test_xml.set(workspace.dmn_xml.unwrap_or_default());
                    set_test_result.set(None);
                    set_test_error.set(None);
                    set_validation_result.set(None);
                    set_validation_error.set(None);
                    set_steps.set(Vec::new());
                    set_assistant_delta.set(String::new());
                    set_agent_activity.set("等待新的建模任务".into());
                    set_version_knowledge_citations.set(workspace.knowledge_citations);
                    set_pending_review.set(None);
                    set_pending_candidate_id.set(None);
                    set_review_confirmed.set(false);
                    set_status.set(RunStatus::Idle);
                    set_generation_mode.set(if has_version {
                        GenerationMode::Modify
                    } else {
                        GenerationMode::Create
                    });
                    set_version_save_status.set(Some("已放弃未保存的生成结果".into()));
                }
                Err(message) => set_project_error.set(Some(message)),
            }
            set_project_busy.set(false);
        });
    };

    let retry = move |_| submit();

    let stop = move |_| {
        if let Some(active) = controller.get() {
            active.abort();
            set_status.set(RunStatus::Aborted);
            set_agent_activity.set("已停止当前生成任务".to_string());
            set_controller.set(None);
        }
    };

    let navigate = leptos_router::hooks::use_navigate();
    let logout = move |_| {
        if let Some(active) = controller.get() {
            active.abort();
        }
        LocalStorage::delete(TOKEN_KEY);
        token.set(None);
        navigate("/login", Default::default());
    };

    let toggle_model_library = move |_| {
        set_workspace_view.set(WorkspaceView::Resources);
        set_resource_view.set(ResourceView::Models);
        let Some(token_value) = token.get() else {
            return;
        };
        set_model_library_busy.set(true);
        set_model_library_error.set(None);
        spawn_local(async move {
            match api::list_model_library(&token_value).await {
                Ok(items) => set_model_library.set(items),
                Err(message) => set_model_library_error.set(Some(message)),
            }
            set_model_library_busy.set(false);
        });
    };

    let toggle_knowledge_library = move |_| {
        set_workspace_view.set(WorkspaceView::Resources);
        set_resource_view.set(ResourceView::Knowledge);
    };

    let select_model_library_item = move |source_kind: String, source_id: String, name: String| {
        set_selected_model_source.set(Some(format!("{source_kind}:{source_id}")));
        set_reuse_project_name.set(name);
        set_reuse_confirmed.set(false);
        set_template_name.set(String::new());
        set_template_industry.set(String::new());
        set_model_library_error.set(None);
    };

    let instantiate_selected_model = move |_| {
        let Some(selected) = selected_model_source.get() else {
            return;
        };
        let Some((source_kind, source_id)) = selected.split_once(':') else {
            return;
        };
        let project_name = reuse_project_name.get().trim().to_string();
        if project_name.is_empty() || !reuse_confirmed.get() || model_library_busy.get() {
            return;
        }
        let Some(token_value) = token.get() else {
            return;
        };
        let source_kind = source_kind.to_string();
        let source_id = source_id.to_string();
        set_model_library_busy.set(true);
        set_model_library_error.set(None);
        spawn_local(async move {
            match api::instantiate_model_library_item(
                &token_value,
                &source_kind,
                &source_id,
                project_name,
                String::new(),
            )
            .await
            {
                Ok(workspace) => {
                    let project = workspace.project.clone();
                    let project_id = project.id;
                    let version = workspace.version.clone();
                    set_projects.update(|items| items.insert(0, project.clone()));
                    set_current_project.set(Some(project));
                    set_current_version.set(version.clone());
                    set_dsl.set(workspace.dsl_yaml.clone());
                    set_source_dsl.set(workspace.dsl_yaml.unwrap_or_default());
                    set_xml.set(workspace.dmn_xml);
                    set_version_knowledge_citations.set(workspace.knowledge_citations);
                    set_generation_mode.set(GenerationMode::Modify);
                    set_workspace_view.set(WorkspaceView::Discovery);
                    set_messages.set(Vec::new());
                    set_steps.set(Vec::new());
                    set_pending_review.set(None);
                    set_review_confirmed.set(false);
                    set_selected_version_id.set(None);
                    set_selected_version_review.set(None);
                    set_review_loading.set(false);
                    set_verification_report.set(None);
                    set_publish_confirmed.set(false);
                    set_release_error.set(None);
                    set_decision_workflow.set(None);
                    set_clarification_answers.set(BTreeMap::new());
                    set_prompt.set(String::new());
                    set_workflow_error.set(None);
                    set_audit_events.set(Vec::new());
                    set_version_save_status
                        .set(Some("已从模型库创建 v1 草稿，验收与批准状态已重置".into()));
                    set_selected_model_source.set(None);
                    set_reuse_confirmed.set(false);
                    if let Some(version) = version {
                        set_versions.set(vec![version]);
                    }
                    match api::get_decision_workflow(&token_value, project_id).await {
                        Ok(workflow) => {
                            let answers = workflow
                                .as_ref()
                                .map(|state| {
                                    state
                                        .answers
                                        .iter()
                                        .map(|answer| {
                                            (answer.clarification_id.clone(), answer.answer.clone())
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            set_clarification_answers.set(answers);
                            set_prompt.set(
                                workflow
                                    .as_ref()
                                    .map(|state| state.requirement_text.clone())
                                    .unwrap_or_default(),
                            );
                            // 新增模型必须从需求与方案页的确认方案入口触发。
                            set_workspace_view.set(WorkspaceView::Discovery);
                            set_decision_workflow.set(workflow);
                        }
                        Err(message) => set_workflow_error.set(Some(message)),
                    }
                    match api::list_project_audit_events(&token_value, project_id).await {
                        Ok(events) => set_audit_events.set(events),
                        Err(message) => set_project_error.set(Some(message)),
                    }
                    set_audit_revision.update(|revision| *revision += 1);
                }
                Err(message) => set_model_library_error.set(Some(message)),
            }
            set_model_library_busy.set(false);
        });
    };

    let save_selected_history_template = move |_| {
        let Some(selected) = selected_model_source.get() else {
            return;
        };
        let Some(item) = model_library
            .get()
            .into_iter()
            .find(|item| format!("{}:{}", item.source_kind, item.source_id) == selected)
        else {
            return;
        };
        let (Some(project_id), Some(version_id), Some(token_value)) =
            (item.source_project_id, item.source_version_id, token.get())
        else {
            return;
        };
        let name = template_name.get().trim().to_string();
        if item.source_kind != "history" || name.is_empty() || model_library_busy.get() {
            return;
        }
        let industry = template_industry.get().trim().to_string();
        let description = item.description.clone();
        set_model_library_busy.set(true);
        set_model_library_error.set(None);
        spawn_local(async move {
            match api::save_decision_template(
                &token_value,
                project_id,
                version_id,
                SaveDecisionTemplate {
                    name,
                    description,
                    industry,
                },
            )
            .await
            {
                Ok(template) => {
                    let key = format!("{}:{}", template.source_kind, template.source_id);
                    set_model_library.update(|items| items.insert(2.min(items.len()), template));
                    set_selected_model_source.set(Some(key));
                    set_template_name.set(String::new());
                    set_template_industry.set(String::new());
                    set_audit_revision.update(|revision| *revision += 1);
                }
                Err(message) => set_model_library_error.set(Some(message)),
            }
            set_model_library_busy.set(false);
        });
    };

    let delete_selected_template = move |_| {
        let Some(selected) = selected_model_source.get() else {
            return;
        };
        let Some((source_kind, source_id)) = selected.split_once(':') else {
            return;
        };
        if source_kind != "template" || model_library_busy.get() {
            return;
        }
        let Some(token_value) = token.get() else {
            return;
        };
        let source_id = source_id.to_string();
        set_model_library_busy.set(true);
        set_model_library_error.set(None);
        spawn_local(async move {
            match api::delete_decision_template(&token_value, &source_id).await {
                Ok(()) => {
                    set_model_library.update(|items| {
                        items.retain(|item| {
                            !(item.source_kind == "template" && item.source_id == source_id)
                        })
                    });
                    set_selected_model_source.set(None);
                    set_reuse_project_name.set(String::new());
                    set_reuse_confirmed.set(false);
                    set_audit_revision.update(|revision| *revision += 1);
                }
                Err(message) => set_model_library_error.set(Some(message)),
            }
            set_model_library_busy.set(false);
        });
    };

    let create_project = move |_| {
        let name = new_project_name.get().trim().to_string();
        if name.is_empty() || project_busy.get() || version_saving.get() {
            return;
        }
        let Some(token_value) = token.get() else {
            return;
        };
        set_project_busy.set(true);
        set_project_error.set(None);
        spawn_local(async move {
            match api::create_project(
                &token_value,
                CreateDecisionProject {
                    name,
                    description: String::new(),
                    risk_level: "medium".to_string(),
                },
            )
            .await
            {
                Ok(project) => {
                    set_projects.update(|items| items.insert(0, project.clone()));
                    set_current_project.set(Some(project));
                    set_current_version.set(None);
                    // 新项目尚未生成需求方案，先引导用户完成需求分析。
                    set_workspace_view.set(WorkspaceView::Discovery);
                    set_new_project_name.set(String::new());
                    set_dsl.set(None);
                    set_source_dsl.set(String::new());
                    set_xml.set(None);
                    set_generation_mode.set(GenerationMode::Create);
                    set_messages.set(Vec::new());
                    set_steps.set(Vec::new());
                    set_decision_workflow.set(None);
                    set_prompt.set(String::new());
                    set_pending_review.set(None);
                    set_review_confirmed.set(false);
                    set_versions.set(Vec::new());
                    set_selected_version_id.set(None);
                    set_selected_version_review.set(None);
                    set_review_loading.set(false);
                    set_audit_events.set(Vec::new());
                    set_rollback_reason.set(String::new());
                    set_rollback_confirmed.set(false);
                    set_rollback_busy.set(false);
                    set_verification_report.set(None);
                    set_verification_busy.set(false);
                    set_publish_confirmed.set(false);
                    set_release_error.set(None);
                    set_clarification_answers.set(BTreeMap::new());
                    set_workflow_error.set(None);
                    set_version_save_status.set(Some("项目已创建，尚未生成草稿".into()));
                    set_audit_revision.update(|revision| *revision += 1);
                }
                Err(message) => set_project_error.set(Some(message)),
            }
            set_project_busy.set(false);
        });
    };

    let delete_current_project = move |_| {
        let Some(project) = current_project.get() else {
            return;
        };
        if project_busy.get() || version_saving.get() || status.get() == RunStatus::Running {
            return;
        }
        let Some(token_value) = token.get() else {
            return;
        };
        let message = format!(
            "确定删除项目“{}”吗？此操作会永久删除其版本、模型产物、验收案例、运行记录和项目成员，且无法恢复。",
            project.name
        );
        let confirmed = web_sys::window()
            .and_then(|window| window.confirm_with_message(&message).ok())
            .unwrap_or(false);
        if !confirmed {
            return;
        }

        let project_id = project.id;
        set_project_busy.set(true);
        set_project_error.set(None);
        spawn_local(async move {
            match api::delete_project(&token_value, project_id).await {
                Ok(()) => {
                    set_projects.update(|items| items.retain(|item| item.id != project_id));
                    set_current_project.set(None);
                    set_current_version.set(None);
                    set_dsl.set(None);
                    set_source_dsl.set(String::new());
                    set_xml.set(None);
                    set_version_knowledge_citations.set(Vec::new());
                    set_pending_knowledge_citations.set(Vec::new());
                    set_generation_mode.set(GenerationMode::Create);
                    set_workspace_view.set(WorkspaceView::Discovery);
                    set_messages.set(Vec::new());
                    set_steps.set(Vec::new());
                    set_assistant_delta.set(String::new());
                    set_error.set(None);
                    set_status.set(RunStatus::Idle);
                    set_decision_workflow.set(None);
                    set_clarification_answers.set(BTreeMap::new());
                    set_prompt.set(String::new());
                    set_workflow_error.set(None);
                    set_pending_review.set(None);
                    set_review_confirmed.set(false);
                    set_versions.set(Vec::new());
                    set_selected_version_id.set(None);
                    set_selected_version_review.set(None);
                    set_review_loading.set(false);
                    set_audit_events.set(Vec::new());
                    set_project_members.set(Vec::new());
                    set_member_error.set(None);
                    set_member_status.set(None);
                    set_rollback_reason.set(String::new());
                    set_rollback_confirmed.set(false);
                    set_rollback_busy.set(false);
                    set_verification_report.set(None);
                    set_publish_confirmed.set(false);
                    set_release_error.set(None);
                    set_decision_test_cases.set(Vec::new());
                    set_test_case_run_summary.set(None);
                    set_test_case_error.set(None);
                    set_test_case_generation_summary.set(None);
                    set_service_run.set(None);
                    set_service_runs.set(Vec::new());
                    set_selected_service_id.set(None);
                    set_decision_analytics.set(None);
                    set_analytics_error.set(None);
                    set_optimization_proposals.set(Vec::new());
                    set_proposal_error.set(None);
                    set_service_error.set(None);
                    set_version_save_status.set(Some("项目已删除，请选择或创建项目".into()));
                }
                Err(message) => set_project_error.set(Some(message)),
            }
            set_project_busy.set(false);
        });
    };

    let select_project = move |event| {
        let Ok(project_id) = event_target_value(&event).parse::<i64>() else {
            return;
        };
        let Some(project) = projects
            .get()
            .into_iter()
            .find(|project| project.id == project_id)
        else {
            return;
        };
        let Some(token_value) = token.get() else {
            return;
        };
        // 项目工作流尚未加载，先停留在需求分析，避免短暂展示错误 Tab。
        set_workspace_view.set(WorkspaceView::Discovery);
        set_decision_workflow.set(None);
        set_clarification_answers.set(BTreeMap::new());
        set_prompt.set(String::new());
        set_workflow_error.set(None);
        set_pending_review.set(None);
        set_review_confirmed.set(false);
        set_versions.set(Vec::new());
        set_selected_version_id.set(None);
        set_selected_version_review.set(None);
        set_review_loading.set(false);
        set_audit_events.set(Vec::new());
        set_rollback_reason.set(String::new());
        set_rollback_confirmed.set(false);
        set_rollback_busy.set(false);
        set_verification_report.set(None);
        set_verification_busy.set(false);
        set_publish_confirmed.set(false);
        set_release_error.set(None);
        set_project_busy.set(true);
        set_project_error.set(None);
        spawn_local(async move {
            match api::get_project_workspace(&token_value, project.id).await {
                Ok(workspace) => {
                    let has_version = workspace.version.is_some();
                    let version = workspace.version.clone();
                    set_current_project.set(Some(workspace.project));
                    set_current_version.set(version.clone());
                    set_dsl.set(workspace.dsl_yaml.clone());
                    set_source_dsl.set(workspace.dsl_yaml.unwrap_or_default());
                    set_xml.set(workspace.dmn_xml);
                    set_version_knowledge_citations.set(workspace.knowledge_citations);
                    set_generation_mode.set(if has_version {
                        GenerationMode::Modify
                    } else {
                        GenerationMode::Create
                    });
                    match api::list_project_versions(&token_value, project.id).await {
                        Ok(items) => set_versions.set(items),
                        Err(message) => set_project_error.set(Some(message)),
                    }
                    match api::list_project_audit_events(&token_value, project.id).await {
                        Ok(events) => set_audit_events.set(events),
                        Err(message) => set_project_error.set(Some(message)),
                    }
                    if let Some(version) = version {
                        match api::get_version_verification(&token_value, project.id, version.id)
                            .await
                        {
                            Ok(report) => set_verification_report.set(report),
                            Err(message) => set_release_error.set(Some(message)),
                        }
                    }
                    set_messages.set(Vec::new());
                    set_steps.set(Vec::new());
                    set_error.set(None);
                    set_version_save_status.set(Some(if has_version {
                        "已恢复最近草稿".into()
                    } else {
                        "项目尚未生成草稿".into()
                    }));
                    match api::get_decision_workflow(&token_value, project.id).await {
                        Ok(workflow) => {
                            let answers = workflow
                                .as_ref()
                                .map(|state| {
                                    state
                                        .answers
                                        .iter()
                                        .map(|answer| {
                                            (answer.clarification_id.clone(), answer.answer.clone())
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            set_clarification_answers.set(answers);
                            set_prompt.set(
                                workflow
                                    .as_ref()
                                    .map(|state| state.requirement_text.clone())
                                    .unwrap_or_default(),
                            );
                            // 项目进入后统一从需求与方案开始；生成模型由确认方案入口触发。
                            set_workspace_view.set(WorkspaceView::Discovery);
                            set_decision_workflow.set(workflow);
                        }
                        Err(message) => set_workflow_error.set(Some(message)),
                    }
                }
                Err(message) => set_project_error.set(Some(message)),
            }
            set_project_busy.set(false);
        });
    };

    let select_version_review = move |event| {
        let raw = event_target_value(&event);
        if raw.is_empty() {
            set_selected_version_id.set(None);
            set_selected_version_review.set(None);
            return;
        }
        let Ok(version_id) = raw.parse::<i64>() else {
            return;
        };
        let Some(project) = current_project.get() else {
            return;
        };
        let Some(token_value) = token.get() else {
            return;
        };
        set_selected_version_id.set(Some(version_id));
        set_workspace_view.set(WorkspaceView::Versions);
        set_selected_version_review.set(None);
        set_review_loading.set(true);
        set_project_error.set(None);
        spawn_local(async move {
            if let Ok(events) = api::list_project_audit_events(&token_value, project.id).await {
                set_audit_events.set(events);
            }
            let is_current_request = || {
                current_project
                    .get_untracked()
                    .is_some_and(|current| current.id == project.id)
                    && selected_version_id.get_untracked() == Some(version_id)
            };
            match api::get_version_review(&token_value, project.id, version_id).await {
                Ok(review) if is_current_request() => {
                    set_selected_version_review.set(Some(review));
                    set_review_loading.set(false);
                }
                Ok(_) => {}
                Err(message) if is_current_request() => {
                    set_project_error.set(Some(message));
                    set_selected_version_id.set(None);
                    set_review_loading.set(false);
                }
                Err(_) => {}
            }
        });
    };

    let create_rollback_draft = move |_| {
        let (Some(project), Some(source_version_id), Some(token_value)) = (
            current_project.get(),
            selected_version_id.get(),
            token.get(),
        ) else {
            return;
        };
        let reason = rollback_reason.get().trim().to_string();
        let source_is_rollback_candidate = versions.get().iter().any(|version| {
            version.id == source_version_id
                && matches!(version.status.as_str(), "published" | "retired")
                && current_version
                    .get()
                    .is_none_or(|current| current.id != version.id)
        });
        if rollback_busy.get()
            || !rollback_confirmed.get()
            || reason.is_empty()
            || !source_is_rollback_candidate
        {
            return;
        }
        set_rollback_busy.set(true);
        set_project_error.set(None);
        set_version_save_status.set(Some("正在从历史版本创建回滚草稿...".into()));
        spawn_local(async move {
            match api::rollback_project_version(&token_value, project.id, source_version_id, reason)
                .await
            {
                Ok(workspace) => {
                    let version = workspace.version.clone();
                    let version_number = version
                        .as_ref()
                        .map(|version| version.version_number)
                        .unwrap_or_default();
                    set_current_project.set(Some(workspace.project));
                    set_current_version.set(version);
                    set_dsl.set(workspace.dsl_yaml.clone());
                    set_source_dsl.set(workspace.dsl_yaml.unwrap_or_default());
                    set_xml.set(workspace.dmn_xml);
                    set_version_knowledge_citations.set(workspace.knowledge_citations);
                    set_generation_mode.set(GenerationMode::Modify);
                    set_workspace_view.set(WorkspaceView::Model);
                    set_pending_review.set(None);
                    set_review_confirmed.set(false);
                    set_verification_report.set(None);
                    set_publish_confirmed.set(false);
                    set_selected_version_id.set(None);
                    set_selected_version_review.set(None);
                    set_rollback_reason.set(String::new());
                    set_rollback_confirmed.set(false);
                    match api::list_project_versions(&token_value, project.id).await {
                        Ok(items) => set_versions.set(items),
                        Err(message) => set_project_error.set(Some(message)),
                    }
                    match api::list_project_audit_events(&token_value, project.id).await {
                        Ok(events) => set_audit_events.set(events),
                        Err(message) => set_project_error.set(Some(message)),
                    }
                    set_version_save_status.set(Some(format!(
                        "回滚草稿 v{version_number} 已创建，请重新运行并批准验收案例"
                    )));
                    set_audit_revision.update(|revision| *revision += 1);
                }
                Err(message) => set_project_error.set(Some(message)),
            }
            set_rollback_busy.set(false);
        });
    };

    let verify_current_version = move |_| {
        let Some(project) = current_project.get() else {
            return;
        };
        let Some(version) = current_version.get() else {
            return;
        };
        let Some(token_value) = token.get() else {
            return;
        };
        if verification_busy.get() || pending_review.get().is_some() {
            return;
        }
        set_verification_busy.set(true);
        set_publish_confirmed.set(false);
        set_release_error.set(None);
        set_version_save_status.set(Some("正在执行自动边界测试和发布门禁...".into()));
        spawn_local(async move {
            match api::verify_project_version(&token_value, project.id, version.id).await {
                Ok(report) => {
                    let passed = report.status == "passed";
                    set_verification_report.set(Some(report));
                    set_current_version.update(|current| {
                        if let Some(current) = current.as_mut() {
                            if current.id == version.id && current.status != "published" {
                                current.status = if passed { "validated" } else { "draft" }.into();
                            }
                        }
                    });
                    set_versions.update(|items| {
                        if let Some(item) = items.iter_mut().find(|item| item.id == version.id) {
                            if item.status != "published" {
                                item.status = if passed { "validated" } else { "draft" }.into();
                            }
                        }
                    });
                    set_version_save_status.set(Some(if passed {
                        "自动验收通过，可以确认发布".into()
                    } else {
                        "自动验收未通过，请检查失败案例".into()
                    }));
                    set_audit_revision.update(|revision| *revision += 1);
                }
                Err(message) => set_release_error.set(Some(message)),
            }
            set_verification_busy.set(false);
        });
    };

    let publish_current_version = move |_| {
        let Some(project) = current_project.get() else {
            return;
        };
        let Some(version) = current_version.get() else {
            return;
        };
        let Some(token_value) = token.get() else {
            return;
        };
        if verification_busy.get()
            || !publish_confirmed.get()
            || !verification_report
                .get()
                .is_some_and(|report| report.status == "passed")
        {
            return;
        }
        set_verification_busy.set(true);
        set_release_error.set(None);
        set_version_save_status.set(Some("正在固化并发布决策版本...".into()));
        spawn_local(async move {
            match api::publish_project_version(&token_value, project.id, version.id).await {
                Ok(published) => {
                    let published_id = published.id;
                    let published_number = published.version_number;
                    set_current_version.set(Some(published.clone()));
                    set_current_project.update(|current| {
                        if let Some(current) = current.as_mut() {
                            current.published_version_id = Some(published_id);
                        }
                    });
                    set_projects.update(|items| {
                        if let Some(item) = items.iter_mut().find(|item| item.id == project.id) {
                            item.published_version_id = Some(published_id);
                        }
                    });
                    set_versions.update(|items| {
                        for item in items.iter_mut() {
                            if item.id == published_id {
                                *item = published.clone();
                            } else if item.status == "published" {
                                item.status = "retired".into();
                            }
                        }
                    });
                    set_publish_confirmed.set(false);
                    set_version_save_status
                        .set(Some(format!("决策版本 v{published_number} 已发布")));
                    set_audit_revision.update(|revision| *revision += 1);
                }
                Err(message) => set_release_error.set(Some(message)),
            }
            set_verification_busy.set(false);
        });
    };

    let mark_version_draft = move |version_id: i64| {
        set_current_version.update(|current| {
            if let Some(current) = current.as_mut()
                && current.id == version_id
                && current.status == "validated"
            {
                current.status = "draft".into();
            }
        });
        set_versions.update(|versions| {
            if let Some(current) = versions.iter_mut().find(|item| item.id == version_id)
                && current.status == "validated"
            {
                current.status = "draft".into();
            }
        });
    };

    let create_business_test_case = move |_| {
        let (Some(project), Some(version), Some(token_value)) =
            (current_project.get(), current_version.get(), token.get())
        else {
            return;
        };
        if test_case_busy.get() || matches!(version.status.as_str(), "published" | "retired") {
            return;
        }
        let name = test_case_name.get().trim().to_string();
        if name.is_empty() {
            set_test_case_error.set(Some("请输入验收案例名称".into()));
            return;
        }
        let input_json = match serde_json::from_str::<serde_json::Value>(&test_case_input.get()) {
            Ok(value) if value.is_object() => value,
            _ => {
                set_test_case_error.set(Some("验收输入必须是有效的 JSON 对象".into()));
                return;
            }
        };
        let expected_output_json =
            match serde_json::from_str::<serde_json::Value>(&test_case_expected.get()) {
                Ok(value)
                    if value
                        .as_object()
                        .is_some_and(|expected| !expected.is_empty()) =>
                {
                    value
                }
                _ => {
                    set_test_case_error.set(Some("期望输出必须是有效的 JSON 对象".into()));
                    return;
                }
            };
        set_test_case_busy.set(true);
        set_test_case_error.set(None);
        set_test_case_run_summary.set(None);
        spawn_local(async move {
            match api::create_decision_test_case(
                &token_value,
                project.id,
                version.id,
                CreateDecisionTestCase {
                    name,
                    input_json,
                    expected_output_json,
                },
            )
            .await
            {
                Ok(test_case) => {
                    set_decision_test_cases.update(|cases| cases.push(test_case));
                    set_test_case_name.set(String::new());
                    set_verification_report.set(None);
                    set_publish_confirmed.set(false);
                    mark_version_draft(version.id);
                    set_audit_revision.update(|revision| *revision += 1);
                    set_version_save_status.set(Some("业务验收案例已保存，请批量运行".into()));
                }
                Err(message) => set_test_case_error.set(Some(message)),
            }
            set_test_case_busy.set(false);
        });
    };

    let generate_business_test_cases = move |_| {
        let (Some(project), Some(version), Some(token_value)) =
            (current_project.get(), current_version.get(), token.get())
        else {
            return;
        };
        if test_case_busy.get() || matches!(version.status.as_str(), "published" | "retired") {
            return;
        }
        set_test_case_busy.set(true);
        set_test_case_error.set(None);
        set_test_case_generation_summary.set(None);
        set_test_case_run_summary.set(None);
        // Invalidate any list request started before generation. Changing a
        // validated version to draft below also reloads the list; without a
        // revision bump, that stale response can overwrite the generated
        // cases returned by this request.
        set_test_case_load_revision.update(|revision| *revision += 1);
        spawn_local(async move {
            match api::generate_decision_test_cases(&token_value, project.id, version.id).await {
                Ok(result)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id)
                        && current_version
                            .get_untracked()
                            .is_some_and(|current| current.id == version.id) =>
                {
                    let generated_count = result.generated_count;
                    let mut cases = result.cases;
                    // The generation response is useful for latency, but the
                    // list endpoint is the persisted source of truth. Read it
                    // once more after the transaction commits so the UI also
                    // picks up cases created by a concurrent generation.
                    if let Ok(refetched) =
                        api::list_decision_test_cases(&token_value, project.id, version.id).await
                    {
                        // Keep the generation response as a fallback. A list
                        // request can briefly observe an empty result while a
                        // replica catches up; never replace visible cases
                        // with that transient empty response.
                        if !refetched.is_empty() {
                            cases = refetched;
                        }
                    }
                    if !current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id)
                        || !current_version
                            .get_untracked()
                            .is_some_and(|current| current.id == version.id)
                        || test_case_context.get_untracked() != Some((project.id, version.id))
                    {
                        set_test_case_busy.set(false);
                        return;
                    }
                    let mut summary = format!(
                        "新增 {} 个，覆盖 {}/{} 条规则，跳过 {} 条，重复 {} 个",
                        result.generated_count,
                        result.covered_rule_count,
                        result.total_rule_count,
                        result.skipped_rule_count,
                        result.duplicate_count,
                    );
                    if !result.notes.is_empty() {
                        summary.push_str("；");
                        summary.push_str(&result.notes.join("；"));
                    }
                    set_test_case_generation_summary.set(Some(summary));
                    // Show the suite whenever the response contains cases,
                    // including an idempotent generation where every
                    // candidate was already persisted and counted as a
                    // duplicate.
                    if generated_count > 0 || !cases.is_empty() {
                        set_verification_report.set(None);
                        set_publish_confirmed.set(false);
                        set_workspace_view.set(WorkspaceView::Validation);
                        mark_version_draft(version.id);
                        set_version_save_status
                            .set(Some("候选案例已生成，请运行并逐项业务批准".into()));
                    } else {
                        set_version_save_status.set(Some("未生成新的候选案例".into()));
                    }
                    set_decision_test_cases.set(cases);
                    set_audit_revision.update(|revision| *revision += 1);
                }
                Ok(_) => {}
                Err(message)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id)
                        && current_version
                            .get_untracked()
                            .is_some_and(|current| current.id == version.id) =>
                {
                    set_test_case_error.set(Some(message));
                }
                Err(_) => {}
            }
            set_test_case_busy.set(false);
        });
    };

    let run_business_test_cases = move |_| {
        let (Some(project), Some(version), Some(token_value)) =
            (current_project.get(), current_version.get(), token.get())
        else {
            return;
        };
        if test_case_busy.get() || matches!(version.status.as_str(), "published" | "retired") {
            return;
        }
        set_test_case_busy.set(true);
        set_test_case_running.set(true);
        set_test_case_load_revision.update(|revision| *revision += 1);
        set_test_case_error.set(None);
        set_test_case_run_summary.set(None);
        spawn_local(async move {
            match api::run_decision_test_cases(&token_value, project.id, version.id).await {
                Ok(cases) => {
                    let passed = cases.iter().filter(|case| case.status == "passed").count();
                    let failed = cases.iter().filter(|case| case.status == "failed").count();
                    let errors = cases.iter().filter(|case| case.status == "error").count();
                    let total = cases.len();
                    set_decision_test_cases.set(cases);
                    set_test_case_run_summary.set(Some(format!(
                        "执行完成：{total} 个案例，{passed} 个通过，{failed} 个不一致，{errors} 个错误"
                    )));
                    set_verification_report.set(None);
                    set_publish_confirmed.set(false);
                    mark_version_draft(version.id);
                    set_version_save_status
                        .set(Some(format!("业务验收执行完成：{passed} / {total} 通过")));
                }
                Err(message) => {
                    set_test_case_run_summary
                        .set(Some("批量执行未完成，请根据错误信息检查后重试".into()));
                    set_test_case_error.set(Some(message));
                }
            }
            set_test_case_running.set(false);
            set_test_case_busy.set(false);
        });
    };

    let approve_business_test_case = move |case_id: i64, approved: bool| {
        let (Some(project), Some(version), Some(token_value)) =
            (current_project.get(), current_version.get(), token.get())
        else {
            return;
        };
        if test_case_busy.get() || matches!(version.status.as_str(), "published" | "retired") {
            return;
        }
        set_test_case_busy.set(true);
        set_test_case_error.set(None);
        set_test_case_run_summary.set(Some(if approved {
            "正在提交批准请求...".into()
        } else {
            "正在撤销批准...".into()
        }));
        spawn_local(async move {
            match api::approve_decision_test_case(
                &token_value,
                project.id,
                version.id,
                case_id,
                approved,
            )
            .await
            {
                Ok(updated) => {
                    set_decision_test_cases.update(|cases| {
                        if let Some(test_case) = cases.iter_mut().find(|case| case.id == case_id) {
                            *test_case = updated;
                        }
                    });
                    set_verification_report.set(None);
                    set_publish_confirmed.set(false);
                    mark_version_draft(version.id);
                    set_audit_revision.update(|revision| *revision += 1);
                    set_test_case_run_summary.set(Some(if approved {
                        "验收案例已批准".into()
                    } else {
                        "验收案例批准已撤销".into()
                    }));
                }
                Err(message) => set_test_case_error.set(Some(message)),
            }
            set_test_case_busy.set(false);
        });
    };

    let select_pending_test_cases = move |_| {
        let ids = decision_test_cases
            .get()
            .into_iter()
            .filter(|test_case| test_case.status == "passed" && test_case.approved_by.is_none())
            .map(|test_case| test_case.id)
            .collect::<HashSet<_>>();
        set_selected_test_case_ids.set(ids);
    };

    let batch_approve_test_cases = move |approved: bool| {
        let (Some(project), Some(version), Some(token_value)) =
            (current_project.get(), current_version.get(), token.get())
        else {
            return;
        };
        let case_ids = selected_test_case_ids.get().into_iter().collect::<Vec<_>>();
        if case_ids.is_empty()
            || test_case_busy.get()
            || matches!(version.status.as_str(), "published" | "retired")
        {
            return;
        }
        set_test_case_busy.set(true);
        set_test_case_error.set(None);
        set_test_case_run_summary.set(None);
        spawn_local(async move {
            let total = case_ids.len();
            let mut updated_count = 0;
            let mut errors = Vec::new();
            for case_id in case_ids {
                match api::approve_decision_test_case(
                    &token_value,
                    project.id,
                    version.id,
                    case_id,
                    approved,
                )
                .await
                {
                    Ok(updated) => {
                        updated_count += 1;
                        set_decision_test_cases.update(|cases| {
                            if let Some(test_case) =
                                cases.iter_mut().find(|case| case.id == case_id)
                            {
                                *test_case = updated;
                            }
                        });
                    }
                    Err(message) => errors.push(message),
                }
            }
            set_selected_test_case_ids.set(HashSet::new());
            if updated_count > 0 {
                set_verification_report.set(None);
                set_publish_confirmed.set(false);
                mark_version_draft(version.id);
                set_audit_revision.update(|revision| *revision += 1);
            }
            if errors.is_empty() {
                set_test_case_run_summary.set(Some(format!(
                    "批量{}完成：{} / {} 个案例",
                    if approved { "批准" } else { "撤销批准" },
                    updated_count,
                    total
                )));
            } else {
                set_test_case_run_summary.set(Some(format!(
                    "批量{}完成：{} / {} 个案例成功，{} 个失败",
                    if approved { "批准" } else { "撤销批准" },
                    updated_count,
                    total,
                    errors.len()
                )));
                set_test_case_error.set(Some(errors.join("；")));
            }
            set_test_case_busy.set(false);
        });
    };

    let batch_approve = move |_| batch_approve_test_cases(true);
    let batch_revoke = move |_| batch_approve_test_cases(false);

    let delete_business_test_case = move |case_id: i64| {
        let (Some(project), Some(version), Some(token_value)) =
            (current_project.get(), current_version.get(), token.get())
        else {
            return;
        };
        if test_case_busy.get() || matches!(version.status.as_str(), "published" | "retired") {
            return;
        }
        set_test_case_busy.set(true);
        set_test_case_error.set(None);
        set_test_case_run_summary.set(None);
        spawn_local(async move {
            match api::delete_decision_test_case(&token_value, project.id, version.id, case_id)
                .await
            {
                Ok(()) => {
                    set_decision_test_cases
                        .update(|cases| cases.retain(|test_case| test_case.id != case_id));
                    set_verification_report.set(None);
                    set_publish_confirmed.set(false);
                    mark_version_draft(version.id);
                }
                Err(message) => set_test_case_error.set(Some(message)),
            }
            set_test_case_busy.set(false);
        });
    };

    let save_member = move |_| {
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            return;
        };
        let username = member_username.get().trim().to_string();
        if username.is_empty() || member_busy.get() {
            return;
        }
        set_member_busy.set(true);
        set_member_error.set(None);
        set_member_status.set(None);
        let role = member_role.get();
        spawn_local(async move {
            match api::save_project_member(&token_value, project.id, &username, role).await {
                Ok(member) => {
                    let saved_role = member.role.clone();
                    let saved_username = member.username.clone();
                    set_project_members.update(|members| {
                        if let Some(existing) = members.iter_mut().find(|item| {
                            item.username == member.username && item.role == member.role
                        }) {
                            *existing = member.clone();
                        } else {
                            members.push(member.clone());
                        }
                        members.sort_by(|left, right| {
                            left.username
                                .cmp(&right.username)
                                .then(left.role.cmp(&right.role))
                        });
                    });
                    match api::list_project_members(&token_value, project.id).await {
                        Ok(members) => {
                            set_project_members.set(members);
                            set_member_status.set(Some(format!(
                                "已为 {saved_username} 添加“{saved_role}”角色"
                            )));
                            set_member_username.set(String::new());
                            set_audit_revision.update(|revision| *revision += 1);
                        }
                        Err(message) => set_member_error
                            .set(Some(format!("角色已保存，但列表刷新失败：{message}"))),
                    }
                }
                Err(message) => set_member_error.set(Some(message)),
            }
            set_member_busy.set(false);
        });
    };

    let remove_member = move |username: String, role: String| {
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            return;
        };
        if member_busy.get() {
            return;
        }
        set_member_busy.set(true);
        set_member_error.set(None);
        set_member_status.set(None);
        spawn_local(async move {
            match api::delete_project_member(&token_value, project.id, &username, Some(&role)).await
            {
                Ok(()) => match api::list_project_members(&token_value, project.id).await {
                    Ok(members) => {
                        set_project_members.set(members);
                        set_member_status.set(Some(format!("已移除 {username} 的“{role}”角色")));
                        set_audit_revision.update(|revision| *revision += 1);
                    }
                    Err(message) => {
                        set_member_error.set(Some(format!("角色已移除，但列表刷新失败：{message}")))
                    }
                },
                Err(message) => set_member_error.set(Some(message)),
            }
            set_member_busy.set(false);
        });
    };

    let review_current_version = move |_| {
        let (Some(token_value), Some(project), Some(version)) =
            (token.get(), current_project.get(), current_version.get())
        else {
            return;
        };
        if verification_busy.get() {
            return;
        }
        set_verification_busy.set(true);
        set_release_error.set(None);
        spawn_local(async move {
            match api::review_project_version(&token_value, project.id, version.id).await {
                Ok(updated) => {
                    set_current_version.set(Some(updated.clone()));
                    set_versions.update(|versions| {
                        if let Some(existing) =
                            versions.iter_mut().find(|item| item.id == updated.id)
                        {
                            *existing = updated;
                        }
                    });
                    set_verification_report.set(None);
                    set_publish_confirmed.set(false);
                    set_version_save_status
                        .set(Some("版本已完成独立复核，请重新执行自动验收".into()));
                    set_audit_revision.update(|revision| *revision += 1);
                }
                Err(message) => set_release_error.set(Some(message)),
            }
            set_verification_busy.set(false);
        });
    };

    let select_published_service = move |event| {
        let service_id = event_target_value(&event);
        let selected = published_services
            .get()
            .into_iter()
            .find(|service| service.id == service_id);
        set_selected_service_id.set(selected.as_ref().map(|service| service.id.clone()));
        set_service_input.set(
            selected
                .as_ref()
                .map(service_input_template)
                .unwrap_or_else(|| "{}".into()),
        );
        set_service_run.set(None);
        set_service_error.set(None);
    };

    let execute_published_service = move |_| {
        let (Some(project), Some(service_id), Some(token_value)) = (
            current_project.get(),
            selected_service_id.get(),
            token.get(),
        ) else {
            return;
        };
        if service_busy.get() {
            return;
        }
        let inputs = match serde_json::from_str::<serde_json::Value>(&service_input.get()) {
            Ok(value) if value.is_object() => value,
            _ => {
                set_service_error.set(Some("服务输入必须是有效的 JSON 对象".into()));
                return;
            }
        };
        let business_reference = service_business_reference.get().trim().to_string();
        set_service_busy.set(true);
        set_service_error.set(None);
        set_service_run.set(None);
        spawn_local(async move {
            match api::execute_published_decision_service(
                &token_value,
                project.id,
                &service_id,
                business_reference,
                inputs,
            )
            .await
            {
                Ok(run) => {
                    set_service_runs.update(|runs| runs.insert(0, run.clone()));
                    set_service_run.set(Some(run));
                    set_service_business_reference.set(String::new());
                    set_analytics_revision.update(|revision| *revision += 1);
                    set_audit_revision.update(|revision| *revision += 1);
                }
                Err(message) => set_service_error.set(Some(message)),
            }
            set_service_busy.set(false);
        });
    };

    let create_service_key = move |_| {
        let (Some(project), Some(service_id), Some(token_value)) = (
            current_project.get(),
            selected_service_id.get(),
            token.get(),
        ) else {
            return;
        };
        let name = service_key_name.get().trim().to_string();
        if name.is_empty() || service_key_busy.get() {
            return;
        }
        set_service_key_busy.set(true);
        set_service_key_error.set(None);
        set_service_key_secret.set(None);
        set_service_key_copy_status.set(CopyStatus::Idle);
        set_service_key_service_id.set(None);
        set_service_curl_copy_status.set(CopyStatus::Idle);
        let service_id_for_request = service_id.clone();
        spawn_local(async move {
            match api::create_decision_service_api_key(&token_value, project.id, service_id, name)
                .await
            {
                Ok(created) => {
                    set_service_keys.update(|keys| keys.insert(0, created.key));
                    set_service_key_secret.set(Some(created.secret));
                    set_service_key_service_id.set(Some(service_id_for_request));
                    set_service_key_name.set(String::new());
                }
                Err(message) => set_service_key_error.set(Some(message)),
            }
            set_service_key_busy.set(false);
        });
    };

    let copy_service_key = move |_| {
        let Some(secret) = service_key_secret.get() else {
            return;
        };
        spawn_local(async move {
            let copied = if let Some(window) = web_sys::window() {
                JsFuture::from(window.navigator().clipboard().write_text(&secret))
                    .await
                    .is_ok()
            } else {
                false
            };
            set_service_key_copy_status.set(if copied {
                CopyStatus::Copied
            } else {
                CopyStatus::Failed
            });
        });
    };

    let copy_service_curl = move |_| {
        let (Some(secret), Some(service_id)) =
            (service_key_secret.get(), service_key_service_id.get())
        else {
            return;
        };
        let Some(service) = published_services
            .get()
            .into_iter()
            .find(|service| service.id == service_id)
        else {
            return;
        };
        let command = public_service_curl(&service, &secret);
        spawn_local(async move {
            let copied = if let Some(window) = web_sys::window() {
                JsFuture::from(window.navigator().clipboard().write_text(&command))
                    .await
                    .is_ok()
            } else {
                false
            };
            set_service_curl_copy_status.set(if copied {
                CopyStatus::Copied
            } else {
                CopyStatus::Failed
            });
        });
    };

    let revoke_service_key = move |key_id: i64| {
        let (Some(project), Some(token_value)) = (current_project.get(), token.get()) else {
            return;
        };
        if service_key_busy.get() {
            return;
        }
        set_service_key_busy.set(true);
        set_service_key_error.set(None);
        spawn_local(async move {
            match api::revoke_decision_service_api_key(&token_value, project.id, key_id).await {
                Ok(()) => set_service_keys.update(|keys| keys.retain(|key| key.id != key_id)),
                Err(message) => set_service_key_error.set(Some(message)),
            }
            set_service_key_busy.set(false);
        });
    };

    let edit_run_outcome = move |run: DecisionServiceRun| {
        set_outcome_run_id.set(Some(run.id));
        set_outcome_business_reference.set(run.business_reference);
        set_outcome_actual_json.set(
            run.actual_outcome_json
                .as_ref()
                .and_then(|value| serde_json::to_string_pretty(value).ok())
                .unwrap_or_else(|| "{}".into()),
        );
        set_outcome_assessment.set(run.outcome_assessment.unwrap_or_else(|| "confirmed".into()));
        set_outcome_note.set(run.outcome_note);
        set_outcome_occurred_at.set(
            run.outcome_occurred_at
                .map(|value| value.format("%Y-%m-%dT%H:%M").to_string())
                .unwrap_or_default(),
        );
        set_service_error.set(None);
    };

    let save_run_outcome = move |_| {
        let (Some(project), Some(run_id), Some(token_value)) =
            (current_project.get(), outcome_run_id.get(), token.get())
        else {
            return;
        };
        let actual_outcome =
            match serde_json::from_str::<serde_json::Value>(&outcome_actual_json.get()) {
                Ok(value) if value.is_object() => value,
                _ => {
                    set_service_error.set(Some("实际业务结果必须是有效的 JSON 对象".into()));
                    return;
                }
            };
        let business_reference = outcome_business_reference.get().trim().to_string();
        if business_reference.is_empty() {
            set_service_error.set(Some("回填实际结果时必须填写业务关联号".into()));
            return;
        }
        let occurred_at = if outcome_occurred_at.get().trim().is_empty() {
            None
        } else {
            match chrono::NaiveDateTime::parse_from_str(
                outcome_occurred_at.get().trim(),
                "%Y-%m-%dT%H:%M",
            ) {
                Ok(value) => Some(
                    value
                        .and_local_timezone(chrono::Local)
                        .single()
                        .map(|value| value.with_timezone(&chrono::Utc))
                        .unwrap_or_else(|| value.and_utc()),
                ),
                Err(_) => {
                    set_service_error.set(Some("实际结果发生时间格式无效".into()));
                    return;
                }
            }
        };
        set_outcome_busy.set(true);
        set_service_error.set(None);
        spawn_local(async move {
            match api::save_decision_run_outcome(
                &token_value,
                project.id,
                run_id,
                SaveDecisionRunOutcome {
                    business_reference,
                    actual_outcome,
                    assessment: outcome_assessment.get(),
                    note: outcome_note.get(),
                    occurred_at,
                },
            )
            .await
            {
                Ok(updated) => {
                    set_service_runs.update(|runs| {
                        if let Some(run) = runs.iter_mut().find(|run| run.id == updated.id) {
                            *run = updated.clone();
                        }
                    });
                    if service_run.get().is_some_and(|run| run.id == updated.id) {
                        set_service_run.set(Some(updated));
                    }
                    set_outcome_run_id.set(None);
                    set_analytics_revision.update(|revision| *revision += 1);
                    set_audit_revision.update(|revision| *revision += 1);
                }
                Err(message) => set_service_error.set(Some(message)),
            }
            set_outcome_busy.set(false);
        });
    };

    let analyze_requirement_action = move || {
        // 已确认方案是后续建模的唯一依据，确认后禁止重新分析，避免覆盖已确认方案。
        if decision_workflow
            .get()
            .as_ref()
            .is_some_and(|workflow| workflow.status == "confirmed")
        {
            return;
        }
        // 刷新页面后输入框可能为空，但已加载的工作流仍保存了原始需求。
        // 重新分析应复用这份需求，避免按钮因输入框未恢复而失效。
        let prompt_text = prompt.get();
        let text = if prompt_text.trim().is_empty() {
            decision_workflow
                .get()
                .map(|workflow| workflow.requirement_text.trim().to_string())
                .unwrap_or_default()
        } else {
            prompt_text.trim().to_string()
        };
        if prompt_text.trim().is_empty() && !text.is_empty() {
            set_prompt.set(text.clone());
        }
        let Some(project) = current_project.get() else {
            set_workflow_error.set(Some("请先创建或选择决策项目".into()));
            return;
        };
        let Some(token_value) = token.get() else {
            return;
        };
        if text.is_empty() || workflow_busy.get() {
            return;
        }
        set_workflow_busy.set(true);
        set_workflow_analyzing.set(true);
        set_workflow_error.set(None);
        set_agent_activity.set("正在分析业务目标、输入输出和关键歧义".into());
        let controller = AbortController::new().expect("AbortController should be available");
        let set_decision_workflow = set_decision_workflow;
        let set_clarification_answers = set_clarification_answers;
        let set_workflow_error = set_workflow_error;
        let set_agent_activity = set_agent_activity;
        let set_workflow_busy = set_workflow_busy;
        let set_workflow_analyzing = set_workflow_analyzing;
        spawn_local(async move {
            let result = api::analyze_requirement_stream(
                &token_value,
                project.id,
                text,
                controller.clone(),
                |event| match event {
                    SseEvent::Step(message) => {
                        set_agent_activity.set(agent_activity_for_step(&message));
                    }
                    SseEvent::WorkflowResult(workflow) => {
                        set_clarification_answers.set(BTreeMap::new());
                        set_decision_workflow.set(Some(workflow));
                        set_agent_activity.set("需求分析完成，请核对摘要和澄清问题".into());
                    }
                    SseEvent::Error(message) => {
                        set_workflow_error.set(Some(message));
                        set_agent_activity.set("需求分析未完成".into());
                    }
                    SseEvent::Delta(_)
                    | SseEvent::Preview(_)
                    | SseEvent::Result(_)
                    | SseEvent::ChangeIntent(_) => {}
                },
            )
            .await;
            if !controller.signal().aborted() {
                if let Err(message) = result {
                    set_workflow_error.set(Some(message));
                    set_agent_activity.set("需求分析未完成".into());
                }
            }
            set_workflow_busy.set(false);
            set_workflow_analyzing.set(false);
        });
    };

    let analyze_requirement = move |_| analyze_requirement_action();

    let create_decision_plan_action = move || {
        let Some(project) = current_project.get() else {
            return;
        };
        let Some(token_value) = token.get() else {
            return;
        };
        if workflow_busy.get() {
            return;
        }
        let answers = clarification_answers
            .get()
            .into_iter()
            .map(|(clarification_id, answer)| ClarificationAnswer {
                clarification_id,
                answer,
            })
            .collect::<Vec<_>>();
        set_workflow_busy.set(true);
        set_workflow_error.set(None);
        set_agent_activity.set("正在规划决策、依赖和验收标准".into());
        spawn_local(async move {
            match api::create_decision_plan(&token_value, project.id, answers).await {
                Ok(workflow) => {
                    set_decision_workflow.set(Some(workflow));
                    set_agent_activity.set("决策方案已生成，等待确认".into());
                }
                Err(message) => {
                    set_workflow_error.set(Some(message));
                    set_agent_activity.set("决策方案规划未完成".into());
                }
            }
            set_workflow_busy.set(false);
        });
    };

    let create_decision_plan = move |_| create_decision_plan_action();

    let confirm_decision_plan_action = move || {
        let Some(project) = current_project.get() else {
            return;
        };
        let Some(token_value) = token.get() else {
            return;
        };
        if workflow_busy.get() {
            return;
        }
        set_workflow_busy.set(true);
        set_workflow_error.set(None);
        spawn_local(async move {
            match api::confirm_decision_plan(&token_value, project.id).await {
                Ok(workflow) => {
                    set_prompt.set(workflow.requirement_text.clone());
                    set_decision_workflow.set(Some(workflow));
                    set_agent_activity.set("方案已确认，可以生成可执行模型".into());
                }
                Err(message) => set_workflow_error.set(Some(message)),
            }
            set_workflow_busy.set(false);
        });
    };

    let confirm_decision_plan = move |_| confirm_decision_plan_action();

    let generate_model_from_plan = move |_| {
        let confirmed = decision_workflow
            .get()
            .as_ref()
            .is_some_and(|workflow| workflow.status == "confirmed" && workflow.plan.is_some());
        if !confirmed || workflow_busy.get() || status.get() == RunStatus::Running {
            return;
        }
        let has_existing_version = current_version.get().is_some();
        if has_existing_version && source_dsl.get().trim().is_empty() {
            set_error.set(Some(
                "当前版本的 DSL 基线尚未加载，无法生成受控修改；请刷新项目后重试".into(),
            ));
            return;
        }
        // Re-running a confirmed plan against an existing draft is a version
        // modification, so it must go through the ChangeIntent preview flow.
        set_generation_mode.set(if has_existing_version {
            GenerationMode::Modify
        } else {
            GenerationMode::Create
        });
        set_plan_generation_requested.set(true);
        set_workspace_view.set(WorkspaceView::Model);
        submit();
    };

    let on_keydown = move |event: KeyboardEvent| {
        if !(event.ctrl_key() || event.meta_key()) || event.key() != "Enter" {
            return;
        }
        event.prevent_default();

        if generation_mode.get() == GenerationMode::Modify {
            submit();
            return;
        }

        match decision_workflow
            .get()
            .as_ref()
            .map(|workflow| workflow.status.as_str())
        {
            None => analyze_requirement_action(),
            Some("analyzed" | "clarification") => create_decision_plan_action(),
            Some("planned") => confirm_decision_plan_action(),
            Some("confirmed") => submit(),
            Some(_) => analyze_requirement_action(),
        }
    };

    let validate_model = move |_| {
        let Some(value) = xml.get() else {
            set_validation_error.set(Some("请先生成 DMN XML，再进行核验".to_string()));
            set_show_test.set(true);
            return;
        };
        set_show_test.set(true);
        set_validation_running.set(true);
        set_validation_result.set(None);
        set_validation_error.set(None);
        set_steps.update(|items| items.push("→ 正在核验 DMN XML 结构和 FEEL...".to_string()));
        let token = token.get();
        spawn_local(async move {
            let Some(token) = token else {
                set_validation_running.set(false);
                return;
            };
            match api::validate_dmn(&token, value.clone()).await {
                Ok(result) if xml.get_untracked().as_deref() == Some(value.as_str()) => {
                    let decision_count = result
                        .summary
                        .as_ref()
                        .and_then(|summary| summary["decisions"].as_u64())
                        .unwrap_or_default();
                    set_validation_result.set(Some(result));
                    set_steps.update(|items| {
                        items.push(format!("✓ DMN 核验通过：{} 个决策", decision_count))
                    });
                }
                Err(message) if xml.get_untracked().as_deref() == Some(value.as_str()) => {
                    set_validation_error.set(Some(message))
                }
                Ok(_) | Err(_) => {}
            }
            if xml.get_untracked().as_deref() == Some(value.as_str()) {
                set_validation_running.set(false);
            }
        });
    };

    let verify_test_model = move || {
        let source = test_xml.get();
        let Some(token) = token.get() else {
            return;
        };
        if source.trim().is_empty() {
            set_test_input.set("{}".to_string());
            set_validation_result.set(None);
            set_validation_error.set(Some("当前没有可核验的 DMN XML，请先生成模型".to_string()));
            return;
        }
        set_analysis_running.set(true);
        set_validation_running.set(true);
        set_validation_result.set(None);
        set_validation_error.set(None);
        spawn_local(async move {
            match api::analyze_dmn_inputs(&token, source.clone()).await {
                Ok(result) if test_xml.get_untracked() == source => {
                    set_test_input.set(result.template);
                    set_validation_result.set(Some(api::ValidateDmnResult {
                        ok: true,
                        summary: result.summary,
                        error: None,
                    }));
                }
                Err(message) if test_xml.get_untracked() == source => {
                    set_test_input.set("{}".to_string());
                    set_validation_error.set(Some(message));
                }
                Ok(_) | Err(_) => {}
            }
            if test_xml.get_untracked() == source {
                set_analysis_running.set(false);
                set_validation_running.set(false);
            }
        });
    };

    let run_test = move |_| {
        let value = test_xml.get();
        if value.trim().is_empty() {
            set_test_error.set(Some("当前没有可执行的 DMN XML，请先生成模型".to_string()));
            return;
        }
        let Some(token) = token.get() else {
            return;
        };
        set_test_running.set(true);
        set_test_error.set(None);
        set_test_result.set(None);
        let inputs = test_input.get();
        spawn_local(async move {
            match api::evaluate_dmn(&token, value.clone(), inputs).await {
                Ok(result) if test_xml.get_untracked() == value => {
                    set_test_result.set(Some(result))
                }
                Err(message) if test_xml.get_untracked() == value => {
                    set_test_error.set(Some(message))
                }
                Ok(_) | Err(_) => {}
            }
            if test_xml.get_untracked() == value {
                set_test_running.set(false);
            }
        });
    };

    view! {
        <main class=move || format!("app-shell {}", theme.get().class())>
            <header class="topbar">
                <div class="brand-lockup">
                    <div class="brand-mark">"D"</div>
                    <div>
                        <p class="eyebrow">"DMN DECISION WORKSPACE"</p>
                        <h1>"智能决策建模工作台"</h1>
                    </div>
                </div>
                <div class="topbar-actions">
                    <div class="run-status-wrap">
                        <span class="toolbar-label">"当前状态"</span>
                        <div class=move || format!("run-status {}", status.get().class())>
                            <span class="status-dot"></span>
                            {move || status.get().label()}
                        </div>
                    </div>
                    <button
                        class="theme-toggle"
                        title=move || theme.get().toggle_label()
                        aria-label=move || theme.get().toggle_label()
                        on:click=move |_| set_theme.update(|current| *current = match *current {
                            Theme::Dark => Theme::Light,
                            Theme::Light => Theme::Dark,
                        })
                    >
                        <span class="theme-toggle-icon" aria-hidden="true">
                            {move || if theme.get() == Theme::Dark { "☼" } else { "☾" }}
                        </span>
                        <span>{move || if theme.get() == Theme::Dark { "浅色" } else { "深色" }}</span>
                    </button>
                    <button
                        class="button button-quiet"
                        title="退出当前账号"
                        on:click=logout
                    >"退出登录"</button>
                    <Show when=move || user_role.get() == "admin">
                        <A href="/admin" class:button=true class:button-quiet=true>"用户管理"</A>
                    </Show>
                    <button class="button button-primary" on:click=validate_model disabled=move || xml.get().is_none() || validation_running.get()>
                        {move || if validation_running.get() { "核验中..." } else { "人工核验 DMN" }}
                    </button>
                </div>
            </header>

            <section
                class=move || if current_project.get().is_none() { "project-bar project-bar-empty" } else { "project-bar" }
                aria-label="决策项目"
            >
                <div class="project-current">
                    <span class="toolbar-label">"当前项目"</span>
                    <strong>{move || current_project.get().map(|project| project.name).unwrap_or_else(|| "尚未选择项目".into())}</strong>
                    <small>{move || current_version.get().map(|version| format!("v{} · {}", version.version_number, version.status)).unwrap_or_else(|| "暂无版本".into())}</small>
                    <Show when=move || current_project.get().is_none()>
                        <span class="project-required-badge">"需要先选择"</span>
                    </Show>
                </div>
                <div class="project-actions">
                    <select
                        id="decision-project-selector"
                        class="project-select"
                        aria-label="切换决策项目"
                        prop:value=move || current_project.get().map(|project| project.id.to_string()).unwrap_or_default()
                        on:change=select_project
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
                        prop:value=move || selected_version_id.get().map(|id| id.to_string()).unwrap_or_default()
                        on:change=select_version_review
                        disabled=move || review_loading.get() || versions.get().is_empty()
                    >
                        <option value="">{move || if review_loading.get() { "读取审查..." } else { "版本变更" }}</option>
                        <For each=move || versions.get() key=|version| version.id let:version>
                            <option value=version.id.to_string()>{format!("v{} · {}", version.version_number, version.status)}</option>
                        </For>
                    </select>
                    <button
                        class=move || if workspace_view.get() == WorkspaceView::Resources && resource_view.get() == ResourceView::Models { "button button-quiet is-active" } else { "button button-quiet" }
                        on:click=toggle_model_library
                        disabled=move || project_busy.get() || version_saving.get()
                    >"模型库"</button>
                    <button
                        class=move || if workspace_view.get() == WorkspaceView::Resources && resource_view.get() == ResourceView::Knowledge { "button button-quiet is-active" } else { "button button-quiet" }
                        on:click=toggle_knowledge_library
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
                    <button class="button button-quiet" on:click=create_project disabled=move || project_busy.get() || version_saving.get() || new_project_name.get().trim().is_empty()>
                        {move || if project_busy.get() { "处理中..." } else { "创建项目" }}
                    </button>
                    <button
                        class="button button-danger"
                        title="永久删除当前项目及其全部数据"
                        on:click=delete_current_project
                        disabled=move || current_project.get().is_none() || project_busy.get() || version_saving.get() || status.get() == RunStatus::Running
                    >"删除当前项目"</button>
                </div>
                <div class="project-status">
                    <span>{move || version_save_status.get().unwrap_or_else(|| "生成结果需确认变更后保存为草稿版本".into())}</span>
                    <Show when=move || project_error.get().is_some()>
                        <strong>{move || project_error.get().unwrap_or_default()}</strong>
                    </Show>
                </div>
            </section>

            <nav class="workspace-view-tabs" aria-label="项目工作区" role="tablist">
                <button
                    class=move || if workspace_view.get() == WorkspaceView::Discovery { "workspace-view-tab is-active" } else { "workspace-view-tab" }
                    on:click=move |_| {
                        set_generation_mode.set(GenerationMode::Create);
                        set_workspace_view.set(WorkspaceView::Discovery);
                    }
                    role="tab"
                    aria-selected=move || workspace_view.get() == WorkspaceView::Discovery
                >"需求与方案"</button>
                <button
                    class=move || if workspace_view.get() == WorkspaceView::Model { "workspace-view-tab is-active" } else { "workspace-view-tab" }
                    on:click=move |_| {
                        if current_version.get().is_some() || dsl.get().is_some() {
                            set_generation_mode.set(GenerationMode::Modify);
                            set_workspace_view.set(WorkspaceView::Model);
                        }
                    }
                    role="tab"
                    aria-selected=move || workspace_view.get() == WorkspaceView::Model
                    disabled=move || current_version.get().is_none() && dsl.get().is_none()
                >"建模"</button>
                <button
                    class=move || if workspace_view.get() == WorkspaceView::Validation { "workspace-view-tab is-active" } else { "workspace-view-tab" }
                    on:click=move |_| set_workspace_view.set(WorkspaceView::Validation)
                    role="tab"
                    aria-selected=move || workspace_view.get() == WorkspaceView::Validation
                    disabled=move || current_version.get().is_none()
                >"验收" <span>{move || decision_test_cases.get().len()}</span></button>
                <button
                    class=move || if workspace_view.get() == WorkspaceView::Services { "workspace-view-tab is-active" } else { "workspace-view-tab" }
                    on:click=move |_| set_workspace_view.set(WorkspaceView::Services)
                    role="tab"
                    aria-selected=move || workspace_view.get() == WorkspaceView::Services
                    disabled=move || current_project.get().is_none_or(|project| project.published_version_id.is_none())
                >"服务" <span>{move || published_services.get().len()}</span></button>
                <button
                    class=move || if workspace_view.get() == WorkspaceView::Analytics { "workspace-view-tab is-active" } else { "workspace-view-tab" }
                    on:click=move |_| set_workspace_view.set(WorkspaceView::Analytics)
                    role="tab"
                    aria-selected=move || workspace_view.get() == WorkspaceView::Analytics
                    disabled=move || current_project.get().is_none_or(|project| project.published_version_id.is_none())
                >"分析" <span>{move || decision_analytics.get().map(|report| report.findings.len()).unwrap_or_default()}</span></button>
                <button
                    class=move || if workspace_view.get() == WorkspaceView::Versions { "workspace-view-tab is-active" } else { "workspace-view-tab" }
                    on:click=move |_| set_workspace_view.set(WorkspaceView::Versions)
                    role="tab"
                    aria-selected=move || workspace_view.get() == WorkspaceView::Versions
                    disabled=move || versions.get().is_empty()
                >"版本" <span>{move || versions.get().len()}</span></button>
                <button
                    class=move || if workspace_view.get() == WorkspaceView::Collaboration { "workspace-view-tab is-active" } else { "workspace-view-tab" }
                    on:click=move |_| set_workspace_view.set(WorkspaceView::Collaboration)
                    role="tab"
                    aria-selected=move || workspace_view.get() == WorkspaceView::Collaboration
                    disabled=move || current_project.get().is_none()
                >"协作" <span>{move || project_members.get().len()}</span></button>
                <button
                    class=move || if workspace_view.get() == WorkspaceView::Resources { "workspace-view-tab is-active" } else { "workspace-view-tab" }
                    on:click=move |_| set_workspace_view.set(WorkspaceView::Resources)
                    role="tab"
                    aria-selected=move || workspace_view.get() == WorkspaceView::Resources
                >"资源"</button>
            </nav>

            <Show when=move || workspace_view.get() == WorkspaceView::Collaboration && current_project.get().is_some()>
                <section class="collaboration-panel" aria-label="项目协作与职责分离">
                    <div class="collaboration-heading">
                        <div>
                            <span class="yaml-kicker">"PROJECT COLLABORATION"</span>
                            <strong>"成员与发布职责"</strong>
                            <small>"负责人负责成员配置和建模；建模、复核、审批由不同角色完成，同一账号可添加多个角色。"</small>
                        </div>
                        <Show when=move || current_version.get().is_some_and(|version| version.reviewed_by.is_none())>
                            <button class="button button-primary" on:click=review_current_version disabled=move || verification_busy.get()>
                                {move || if verification_busy.get() { "处理中..." } else { "独立复核当前版本" }}
                            </button>
                        </Show>
                    </div>
                    <div class="member-role-guide">
                        <span><b>建模者</b>" 需求、模型与案例"</span>
                        <span><b>复核者</b>" 独立复核、批准案例、执行验收"</span>
                        <span><b>审批人</b>" 在门禁通过后发布"</span>
                        <span><b>查看者</b>" 只读访问"</span>
                    </div>
                    <div class="member-editor">
                        <label><span>"成员用户名"</span><input maxlength="255" placeholder="管理员已添加的用户名" prop:value=member_username on:input=move |event| set_member_username.set(event_target_value(&event)) /></label>
                        <label><span>"角色"</span><select prop:value=member_role on:change=move |event| set_member_role.set(event_target_value(&event))><option value="modeler">"建模者"</option><option value="reviewer">"复核者"</option><option value="approver">"审批人"</option><option value="viewer">"查看者"</option></select></label>
                        <button class="button button-quiet" on:click=save_member disabled=move || member_busy.get() || member_username.get().trim().is_empty()>{move || if member_busy.get() { "处理中..." } else { "添加或更新角色" }}</button>
                    </div>
                    <div class="member-list">
                        <For each=move || project_members.get() key=|member| format!("{}:{}", member.username, member.role) let:member>
                            {
                                let username = member.username.clone();
                                let role = member.role.clone();
                                view! {
                                    <article class="member-row">
                                        <div><strong>{member.username}</strong><small>{format!("{} 添加", member.added_by)}</small></div>
                                        <code>{member.role}</code>
                                        <button class="button button-quiet danger-action" on:click=move |_| remove_member(username.clone(), role.clone()) disabled=move || member_busy.get()>"移除角色"</button>
                                    </article>
                                }
                            }
                        </For>
                        <Show when=move || project_members.get().is_empty()><div class="member-empty">"尚未添加协作成员"</div></Show>
                    </div>
                    <Show when=move || member_status.get().is_some()>
                        <div class="member-status" role="status">{move || member_status.get().unwrap_or_default()}</div>
                    </Show>
                    <Show when=move || member_error.get().is_some()>
                        <div class="member-error" role="alert">
                            <strong>"成员角色保存失败"</strong>
                            <span>{move || member_error.get().unwrap_or_default()}</span>
                        </div>
                    </Show>
                </section>
            </Show>

            <Show when=move || workspace_view.get() == WorkspaceView::Model && !version_knowledge_citations.get().is_empty()>
                <details class="knowledge-citation-strip" aria-label="当前版本知识引用">
                    <summary><strong>"当前版本知识依据"</strong><span>{move || version_knowledge_citations.get().len()}</span></summary>
                    <div>
                        <For each=move || version_knowledge_citations.get() key=|citation| citation.source_id let:citation>
                            <span title=citation.source_uri.clone()>{format!("{} · {}{}", knowledge_type_label(&citation.source_type), citation.name, if citation.version_label.is_empty() { String::new() } else { format!(" v{}", citation.version_label) })}</span>
                        </For>
                    </div>
                </details>
            </Show>

            <Show when=move || workspace_view.get() == WorkspaceView::Resources>
                <div class="resource-library-page">
                    <div class="resource-library-heading">
                        <div>
                            <span class="yaml-kicker">"RESOURCE CENTER"</span>
                            <strong>"决策资源中心"</strong>
                        </div>
                        <div class="resource-library-switch" role="tablist" aria-label="资源类型">
                            <button
                                class=move || if resource_view.get() == ResourceView::Knowledge { "is-active" } else { "" }
                                on:click=toggle_knowledge_library
                                role="tab"
                                aria-selected=move || resource_view.get() == ResourceView::Knowledge
                            >"知识库"</button>
                            <button
                                class=move || if resource_view.get() == ResourceView::Models { "is-active" } else { "" }
                                on:click=toggle_model_library
                                role="tab"
                                aria-selected=move || resource_view.get() == ResourceView::Models
                            >"模型库"</button>
                        </div>
                    </div>

            <Show when=move || resource_view.get() == ResourceView::Knowledge>
                <KnowledgeLibraryPanel token=token current_project=current_project />
            </Show>

            <Show when=move || resource_view.get() == ResourceView::Models>
                <section class="model-library-panel" aria-label="行业模板与历史模型复用">
                    <div class="model-library-heading">
                        <div>
                            <span class="yaml-kicker">"MODEL LIBRARY"</span>
                            <strong>"从可信快照开始"</strong>
                            <small>{move || format!("{} 个可用模型", model_library.get().len())}</small>
                        </div>
                        <div class="model-library-toolbar">
                            <input
                                class="resource-search"
                                type="search"
                                placeholder="搜索模型名称、行业或说明"
                                prop:value=model_library_query
                                on:input=move |event| set_model_library_query.set(event_target_value(&event))
                            />
                            <div class="model-library-filters" role="group" aria-label="模型来源筛选">
                                <For
                                    each=move || vec![("all", "全部"), ("builtin", "行业模板"), ("template", "我的模板"), ("history", "发布历史")]
                                    key=|item| item.0
                                    let:item
                                >
                                    <button
                                        class=move || if model_library_filter.get() == item.0 { "model-library-filter is-active" } else { "model-library-filter" }
                                        on:click=move |_| set_model_library_filter.set(item.0.to_string())
                                    >{item.1}</button>
                                </For>
                            </div>
                        </div>
                    </div>
                    <div class="model-library-body">
                        <div class="model-library-catalog">
                            <Show
                                when=move || !model_library_busy.get() || !model_library.get().is_empty()
                                fallback=move || view! { <div class="model-library-empty">"正在读取模型库..."</div> }
                            >
                                <For
                                    each=move || {
                                        let filter = model_library_filter.get();
                                        let query = model_library_query.get().trim().to_lowercase();
                                        model_library.get().into_iter().filter(|item| {
                                            (filter == "all" || item.source_kind == filter)
                                                && (query.is_empty()
                                                    || item.name.to_lowercase().contains(&query)
                                                    || item.industry.to_lowercase().contains(&query)
                                                    || item.description.to_lowercase().contains(&query))
                                        }).collect::<Vec<_>>()
                                    }
                                    key=|item| format!("{}:{}", item.source_kind, item.source_id)
                                    let:item
                                >
                                    {
                                        let source_key = format!("{}:{}", item.source_kind, item.source_id);
                                        let source_key_for_class = source_key.clone();
                                        let source_kind = item.source_kind.clone();
                                        let source_id = item.source_id.clone();
                                        let name = item.name.clone();
                                        let source_label = match item.source_kind.as_str() {
                                            "builtin" => "行业模板",
                                            "template" => "我的模板",
                                            _ => "发布历史",
                                        };
                                        view! {
                                            <button
                                                class=move || if selected_model_source.get().as_deref() == Some(source_key_for_class.as_str()) { "model-library-item is-selected" } else { "model-library-item" }
                                                on:click=move |_| select_model_library_item(source_kind.clone(), source_id.clone(), name.clone())
                                            >
                                                <span class=format!("model-source source-{}", item.source_kind)>{source_label}</span>
                                                <strong>{item.name}</strong>
                                                <small>{if item.industry.is_empty() { "未分类".into() } else { item.industry }}</small>
                                                <p>{item.description}</p>
                                                <div>
                                                    <span>{format!("风险 {}", item.risk_level)}</span>
                                                    <span>{format!("{} 个业务案例", item.test_case_count)}</span>
                                                </div>
                                            </button>
                                        }
                                    }
                                </For>
                                <Show when=move || {
                                    let filter = model_library_filter.get();
                                    let query = model_library_query.get().trim().to_lowercase();
                                    !model_library_busy.get() && !model_library.get().iter().any(|item| {
                                        (filter == "all" || item.source_kind == filter)
                                            && (query.is_empty()
                                                || item.name.to_lowercase().contains(&query)
                                                || item.industry.to_lowercase().contains(&query)
                                                || item.description.to_lowercase().contains(&query))
                                    })
                                }>
                                    <div class="model-library-empty">"没有匹配的模型"</div>
                                </Show>
                            </Show>
                        </div>
                        <aside class="model-library-action">
                            {move || selected_model_source.get().and_then(|selected| {
                                model_library.get().into_iter().find(|item| format!("{}:{}", item.source_kind, item.source_id) == selected)
                            }).map(|item| {
                                let is_history = item.source_kind == "history";
                                let is_saved = item.source_kind == "template";
                                view! {
                                    <div class="model-library-selection">
                                        <span class="toolbar-label">"已选择"</span>
                                        <strong>{item.name}</strong>
                                        <small>{item.source_project_name.map(|name| match item.source_version_number {
                                            Some(number) => format!("来源：{name} v{number}"),
                                            None => format!("来源：{name}"),
                                        }).unwrap_or_else(|| "平台内置并随服务版本发布".into())}</small>
                                    </div>
                                    <div class="model-library-metadata">
                                        <span>{if item.industry.is_empty() { "未分类".into() } else { item.industry }}</span>
                                        <span>{format!("风险 {}", item.risk_level)}</span>
                                        <span>{format!("{} 个案例", item.test_case_count)}</span>
                                    </div>
                                    <p class="model-library-description">{item.description}</p>
                                    <label class="model-library-field">
                                        <span>"新项目名称"</span>
                                        <input
                                            type="text"
                                            maxlength="120"
                                            prop:value=reuse_project_name
                                            on:input=move |event| {
                                                set_reuse_project_name.set(event_target_value(&event));
                                                set_reuse_confirmed.set(false);
                                            }
                                        />
                                    </label>
                                    <label class="model-library-confirmation">
                                        <input
                                            type="checkbox"
                                            prop:checked=reuse_confirmed
                                            on:change=move |event| set_reuse_confirmed.set(event_target_checked(&event))
                                        />
                                        <span>"确认创建独立草稿，并重新执行测试、批准和发布门禁"</span>
                                    </label>
                                    <button
                                        class="button button-primary"
                                        on:click=instantiate_selected_model
                                        disabled=move || model_library_busy.get() || !reuse_confirmed.get() || reuse_project_name.get().trim().is_empty()
                                    >{move || if model_library_busy.get() { "处理中..." } else { "创建复用项目" }}</button>
                                    <Show when=move || is_history>
                                        <div class="model-template-save">
                                            <span class="toolbar-label">"保存为个人模板"</span>
                                            <input
                                                type="text"
                                                maxlength="120"
                                                placeholder="模板名称"
                                                prop:value=template_name
                                                on:input=move |event| set_template_name.set(event_target_value(&event))
                                            />
                                            <input
                                                type="text"
                                                maxlength="80"
                                                placeholder="行业标签"
                                                prop:value=template_industry
                                                on:input=move |event| set_template_industry.set(event_target_value(&event))
                                            />
                                            <button
                                                class="button button-quiet"
                                                on:click=save_selected_history_template
                                                disabled=move || model_library_busy.get() || template_name.get().trim().is_empty()
                                            >"保存快照"</button>
                                        </div>
                                    </Show>
                                    <Show when=move || is_saved>
                                        <button class="button button-quiet danger-action" on:click=delete_selected_template disabled=move || model_library_busy.get()>
                                            "删除个人模板"
                                        </button>
                                    </Show>
                                }
                            })}
                            <Show when=move || selected_model_source.get().is_none()>
                                <div class="model-library-empty">"选择一个模型查看复用操作"</div>
                            </Show>
                            <Show when=move || model_library_error.get().is_some()>
                                <div class="model-library-error">{move || model_library_error.get().unwrap_or_default()}</div>
                            </Show>
                        </aside>
                    </div>
                </section>
            </Show>
                </div>
            </Show>

            <Show when=move || workspace_view.get() == WorkspaceView::Versions && current_project.get().is_some() && !versions.get().is_empty()>
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
                        <div class="audit-timeline" aria-label="项目审计记录">
                            <Show
                                when=move || !audit_events.get().is_empty()
                                fallback=move || view! { <div class="audit-empty">"暂无审计事件"</div> }
                            >
                                <For each=move || audit_events.get() key=|event| event.id let:event>
                                    {
                                        let kind = audit_event_kind(&event.event_type);
                                        let label = audit_event_type_label(&event.event_type);
                                        view! {
                                            <article class=format!("audit-event audit-type-{kind}")>
                                                <span class="audit-event-dot" aria-hidden="true"></span>
                                                <div>
                                                    <strong>{event.summary}</strong>
                                                    <small>{format!("{} · {}", event.actor_username, event.created_at.format("%m-%d %H:%M:%S"))}</small>
                                                    <code>{label}</code>
                                                </div>
                                            </article>
                                        }
                                    }
                                </For>
                            </Show>
                        </div>
                    </div>
                </section>
            </Show>

            <Show when=move || workspace_view.get() == WorkspaceView::Versions && selected_version_review.get().is_some()>
                <section class="version-history-panel">
                    <div class="version-history-heading">
                        <div><span class="yaml-kicker">"VERSION HISTORY"</span><strong>"已保存版本审查"</strong></div>
                        <button class="button button-quiet" on:click=move |_| {
                            set_selected_version_id.set(None);
                            set_selected_version_review.set(None);
                        }>"关闭"</button>
                    </div>
                    {move || selected_version_review.get().map(|review| view! { <SavedVersionReviewView review=review /> })}
                    <Show when=move || {
                        selected_version_id.get().is_some_and(|selected_id| {
                            versions.get().iter().any(|version| {
                                version.id == selected_id
                                    && matches!(version.status.as_str(), "published" | "retired")
                                    && current_version.get().is_none_or(|current| current.id != version.id)
                            })
                        })
                    }>
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
                                disabled=move || rollback_busy.get()
                            ></textarea>
                            <label class="rollback-confirmation">
                                <input
                                    type="checkbox"
                                    prop:checked=rollback_confirmed
                                    on:change=move |event| set_rollback_confirmed.set(event_target_checked(&event))
                                    disabled=move || rollback_busy.get()
                                />
                                <span>"我已确认回滚差异，并接受重新验收后才能发布"</span>
                            </label>
                            <button
                                class="button button-primary"
                                on:click=create_rollback_draft
                                disabled=move || rollback_busy.get() || !rollback_confirmed.get() || rollback_reason.get().trim().is_empty()
                            >
                                {move || if rollback_busy.get() { "创建中..." } else { "创建回滚草稿" }}
                            </button>
                        </div>
                    </Show>
                </section>
            </Show>

            <Show when=move || workspace_view.get() == WorkspaceView::Services && current_project.get().is_some_and(|project| project.published_version_id.is_some())>
                <section class="published-services-panel" aria-label="已发布决策服务">
                    <div class="published-services-heading">
                        <div>
                            <span class="yaml-kicker">"PUBLISHED SERVICES"</span>
                            <strong>"已发布决策服务"</strong>
                            <small>{move || format!("{} 个可调用服务", published_services.get().len())}</small>
                        </div>
                        <span class="service-live-status">"LIVE"</span>
                    </div>
                    <div class="service-public-access">
                        <div class="service-public-access-heading">
                            <div>
                                <strong>"对外调用凭证"</strong>
                                <small>"无需登录，凭证仅绑定当前决策服务；发布新版本后仍按线上版本执行"</small>
                            </div>
                            <span>"X-Decision-Key"</span>
                        </div>
                        <div class="service-key-create">
                            <input
                                type="text"
                                maxlength="120"
                                placeholder="凭证名称，例如 order-service"
                                prop:value=service_key_name
                                on:input=move |event| set_service_key_name.set(event_target_value(&event))
                                disabled=move || service_key_busy.get() || selected_service_id.get().is_none()
                            />
                            <button
                                class="button button-primary"
                                on:click=create_service_key
                                disabled=move || service_key_busy.get() || selected_service_id.get().is_none() || service_key_name.get().trim().is_empty()
                            >{move || if service_key_busy.get() { "处理中..." } else { "生成调用凭证" }}</button>
                        </div>
                        <Show when=move || service_key_secret.get().is_some()>
                            <div class="service-key-secret" role="alert">
                                <div class="service-key-secret-heading">
                                    <strong>"完整密钥，仅显示一次"</strong>
                                    <button
                                        type="button"
                                        class="button button-quiet"
                                        title="复制完整密钥"
                                        aria-label="复制完整密钥"
                                        on:click=copy_service_key
                                    >
                                        {move || match service_key_copy_status.get() {
                                            CopyStatus::Idle => "复制完整密钥",
                                            CopyStatus::Copied => "已复制",
                                            CopyStatus::Failed => "复制失败",
                                        }}
                                    </button>
                                </div>
                                <input
                                    class="service-key-secret-value"
                                    type="text"
                                    readonly=true
                                    spellcheck="false"
                                    aria-label="完整决策服务调用密钥"
                                    prop:value=move || service_key_secret.get().unwrap_or_default()
                                />
                                <div class="service-key-curl-heading">
                                    <strong>"可直接执行的 curl"</strong>
                                    <button
                                        type="button"
                                        class="button button-quiet"
                                        title="复制可执行 curl"
                                        aria-label="复制可执行 curl"
                                        on:click=copy_service_curl
                                    >
                                        {move || match service_curl_copy_status.get() {
                                            CopyStatus::Idle => "复制 curl",
                                            CopyStatus::Copied => "已复制",
                                            CopyStatus::Failed => "复制失败",
                                        }}
                                    </button>
                                </div>
                                <pre class="service-key-curl"><code>{move || {
                                    let secret = service_key_secret.get().unwrap_or_default();
                                    let service_id = service_key_service_id.get();
                                    published_services.get().into_iter()
                                        .find(|service| Some(service.id.clone()) == service_id)
                                        .map(|service| public_service_curl(&service, &secret))
                                        .unwrap_or_default()
                                }}</code></pre>
                            </div>
                        </Show>
                        <Show when=move || service_key_error.get().is_some()>
                            <div class="service-key-error">{move || service_key_error.get().unwrap_or_default()}</div>
                        </Show>
                        <Show when=move || !service_keys.get().is_empty()>
                            <div class="service-key-list">
                                <For each=move || service_keys.get() key=|key| key.id let:key>
                                    <div class=format!("service-key-row{}", if key.revoked_at.is_some() { " service-key-revoked" } else { "" })>
                                        <div>
                                            <strong>{key.name.clone()}</strong>
                                            <small>{format!("凭证 ID：{} · 服务：{} · 仅用于识别，不能直接调用", key.key_prefix, key.service_id)}</small>
                                        </div>
                                        <span>{move || if key.revoked_at.is_some() { "已撤销" } else { "有效" }}</span>
                                        <button
                                            type="button"
                                            class="button button-quiet danger-action"
                                            title="删除并立即停用凭证"
                                            aria-label="删除并立即停用凭证"
                                            on:click=move |_| revoke_service_key(key.id)
                                            disabled=move || service_key_busy.get() || key.revoked_at.is_some()
                                        >"删除"</button>
                                    </div>
                                </For>
                            </div>
                        </Show>
                    </div>
                    <Show
                        when=move || !published_services.get().is_empty()
                        fallback=move || view! { <div class="service-empty">"当前发布版本没有可调用决策服务"</div> }
                    >
                        <div class="service-contract-list">
                            <For each=move || published_services.get() key=|service| service.id.clone() let:service>
                                {let endpoint = public_service_endpoint(&service);
                                let endpoint_service_id = service.id.clone();
                                let endpoint_for_copy = endpoint.clone();
                                let service_id_for_copy = endpoint_service_id.clone();
                                let service_id_for_label = endpoint_service_id.clone();
                                let copy_endpoint = move |_| {
                                    let endpoint = endpoint_for_copy.clone();
                                    let service_id = service_id_for_copy.clone();
                                    spawn_local(async move {
                                        let copied = if let Some(window) = web_sys::window() {
                                            JsFuture::from(window.navigator().clipboard().write_text(&endpoint))
                                                .await
                                                .is_ok()
                                        } else {
                                            false
                                        };
                                        if copied {
                                            set_service_endpoint_copied.set(Some(service_id));
                                        }
                                    });
                                };
                                view! { <article class="service-contract">
                                    <div><strong>{service.name.clone()}</strong><span>{format!("v{}", service.version_number)}</span></div>
                                    <div class="service-endpoint">
                                        <code>{format!("POST {endpoint}")}</code>
                                        <button
                                            type="button"
                                            class="button button-quiet"
                                            title="复制公开调用 URL"
                                            aria-label="复制公开调用 URL"
                                            on:click=copy_endpoint
                                        >{move || if service_endpoint_copied.get().as_deref() == Some(service_id_for_label.as_str()) { "已复制" } else { "复制 URL" }}</button>
                                    </div>
                                    <div class="service-fields">
                                        <div>
                                            <small>"输入"</small>
                                            <For each=move || service.inputs.clone() key=|field| format!("{}:{}", field.source, field.name) let:field>
                                                <span><strong>{field.name}</strong><em>{field.type_ref}</em></span>
                                            </For>
                                        </div>
                                        <div>
                                            <small>"输出"</small>
                                            <For each=move || service.outputs.clone() key=|field| field.name.clone() let:field>
                                                <span><strong>{field.name}</strong><em>{field.type_ref}</em></span>
                                            </For>
                                        </div>
                                    </div>
                                </article> } }
                            </For>
                        </div>
                        <div class="service-runner">
                            <div class="service-runner-controls">
                                <label>
                                    <span>"决策服务"</span>
                                    <select
                                        prop:value=move || selected_service_id.get().unwrap_or_default()
                                        on:change=select_published_service
                                        disabled=move || service_busy.get()
                                    >
                                        <For each=move || published_services.get() key=|service| service.id.clone() let:service>
                                            <option value=service.id>{service.name}</option>
                                        </For>
                                    </select>
                                </label>
                                <label>
                                    <span>"业务关联号"</span>
                                    <input
                                        type="text"
                                        maxlength="255"
                                        placeholder="订单号、申请号或案件号"
                                        prop:value=service_business_reference
                                        on:input=move |event| set_service_business_reference.set(event_target_value(&event))
                                        disabled=move || service_busy.get()
                                    />
                                </label>
                                <label class="service-input-editor">
                                    <span>"调用输入 JSON"</span>
                                    <textarea
                                        rows="5"
                                        spellcheck="false"
                                        prop:value=service_input
                                        on:input=move |event| set_service_input.set(event_target_value(&event))
                                    ></textarea>
                                </label>
                                <button
                                    class="button button-primary"
                                    on:click=execute_published_service
                                    disabled=move || service_busy.get() || selected_service_id.get().is_none() || service_business_reference.get().trim().is_empty()
                                >{move || if service_busy.get() { "调用中..." } else { "调用服务" }}</button>
                            </div>
                            <Show when=move || service_error.get().is_some()>
                                <div class="service-run-error">{move || service_error.get().unwrap_or_default()}</div>
                            </Show>
                            {move || service_run.get().map(|run| {
                                let output = serde_json::to_string_pretty(&run.output_json).unwrap_or_default();
                                let trace = serde_json::from_value::<ExecutionTrace>(run.execution_trace_json).ok();
                                view! {
                                    <div class=format!("service-run-result run-{}", run.status)>
                                        <div class="service-run-summary">
                                            <strong>{format!("运行 #{} · {}", run.id, run.service_name)}</strong>
                                            <span>{format!("{} · {} ms", run.status, run.duration_ms)}</span>
                                        </div>
                                        <small class="service-business-reference">{format!("业务关联号：{}", run.business_reference)}</small>
                                        <code>{output}</code>
                                        {trace.map(|trace| view! { <ExecutionTraceView trace=trace /> })}
                                    </div>
                                }
                            })}
                            <Show when=move || !service_runs.get().is_empty()>
                                <div class="outcome-collection-summary">
                                    <div><strong>{move || service_runs.get().iter().filter(|run| run.status == "succeeded").count()}</strong><span>"成功运行"</span></div>
                                    <div><strong>{move || service_runs.get().iter().filter(|run| run.outcome_assessment.is_some()).count()}</strong><span>"已回填"</span></div>
                                    <div class="has-mismatch"><strong>{move || service_runs.get().iter().filter(|run| run.outcome_assessment.as_deref() == Some("mismatch")).count()}</strong><span>"结果偏差"</span></div>
                                </div>
                                <details class="service-run-history">
                                    <summary>{move || format!("运行反馈与实际结果（最近 {} 条）", service_runs.get().len())}</summary>
                                    <div class="service-run-history-list">
                                        <For each=move || recent_service_runs(service_runs.get()) key=|run| run.id let:run>
                                            {
                                                let run_id = run.id;
                                                let run_for_edit = RwSignal::new(run.clone());
                                                let input = serde_json::to_string_pretty(&run.input_json).unwrap_or_default();
                                                let output = serde_json::to_string_pretty(&run.output_json).unwrap_or_default();
                                                let actual_outcome = run.actual_outcome_json.as_ref().and_then(|value| serde_json::to_string_pretty(value).ok());
                                                let trace = serde_json::from_value::<ExecutionTrace>(run.execution_trace_json).ok();
                                                let assessment = run.outcome_assessment.clone();
                                                let has_assessment = assessment.is_some();
                                                let can_edit_outcome = run.status == "succeeded";
                                                let outcome_occurred_label = run.outcome_occurred_at.map(|value| format!("结果发生：{}", value.format("%Y-%m-%d %H:%M"))).unwrap_or_else(|| "未记录结果发生时间".into());
                                                let outcome_reporter_label = run.outcome_reported_by.as_ref().map(|actor| format!("回填：{}", actor)).unwrap_or_default();
                                                let outcome_note_text = (!run.outcome_note.is_empty()).then(|| run.outcome_note.clone());
                                                let assessment_class = format!("outcome-state outcome-{}", assessment.as_deref().unwrap_or("pending"));
                                                view! {
                                                    <details class=format!("service-history-run run-{}", run.status)>
                                                        <summary>
                                                            <strong>{format!("#{} · {}", run.id, run.service_name)}</strong>
                                                            <span class=assessment_class>{outcome_assessment_label(assessment.as_deref())}</span>
                                                            <small>{format!("{} ms · {}", run.duration_ms, run.executed_at.format("%Y-%m-%d %H:%M:%S"))}</small>
                                                        </summary>
                                                        <div class="service-run-reference">{if run.business_reference.is_empty() { "未设置业务关联号".into() } else { format!("业务关联号：{}", run.business_reference) }}</div>
                                                        <Show when=move || assessment.is_some()>
                                                            <div class="outcome-report-meta">
                                                                <span>{outcome_occurred_label.clone()}</span>
                                                                <span>{outcome_reporter_label.clone()}</span>
                                                                {outcome_note_text.clone().map(|note| view! { <p>{note}</p> })}
                                                            </div>
                                                        </Show>
                                                        <div class="service-history-contract">
                                                            <div><small>"输入"</small><code>{input}</code></div>
                                                            <div><small>"决策输出"</small><code>{output}</code></div>
                                                            {actual_outcome.map(|actual| view! { <div><small>"实际业务结果"</small><code>{actual}</code></div> })}
                                                        </div>
                                                        <Show when=move || can_edit_outcome && outcome_run_id.get() != Some(run_id)>
                                                            <button class="button button-quiet outcome-edit-button" on:click=move |_| edit_run_outcome(run_for_edit.get())>
                                                                {if has_assessment { "修改实际结果" } else { "回填实际结果" }}
                                                            </button>
                                                        </Show>
                                                        <Show when=move || outcome_run_id.get() == Some(run_id)>
                                                            <div class="outcome-editor">
                                                                <div class="outcome-editor-grid">
                                                                    <label><span>"业务关联号"</span><input maxlength="255" prop:value=outcome_business_reference on:input=move |event| set_outcome_business_reference.set(event_target_value(&event)) /></label>
                                                                    <label><span>"结果发生时间"</span><input type="datetime-local" prop:value=outcome_occurred_at on:input=move |event| set_outcome_occurred_at.set(event_target_value(&event)) /></label>
                                                                    <label><span>"结果判定"</span><select prop:value=outcome_assessment on:change=move |event| set_outcome_assessment.set(event_target_value(&event))><option value="confirmed">"符合决策"</option><option value="mismatch">"存在偏差"</option><option value="inconclusive">"无法判断"</option></select></label>
                                                                    <label class="outcome-editor-wide"><span>"实际业务结果 JSON"</span><textarea rows="4" spellcheck="false" prop:value=outcome_actual_json on:input=move |event| set_outcome_actual_json.set(event_target_value(&event))></textarea></label>
                                                                    <label class="outcome-editor-wide"><span>"反馈说明"</span><textarea rows="2" maxlength="4000" prop:value=outcome_note on:input=move |event| set_outcome_note.set(event_target_value(&event))></textarea></label>
                                                                </div>
                                                                <div class="outcome-editor-actions">
                                                                    <button class="button button-quiet" on:click=move |_| set_outcome_run_id.set(None) disabled=move || outcome_busy.get()>"取消"</button>
                                                                    <button class="button button-primary" on:click=save_run_outcome disabled=move || outcome_busy.get() || outcome_business_reference.get().trim().is_empty()>{move || if outcome_busy.get() { "保存中..." } else { "保存实际结果" }}</button>
                                                                </div>
                                                            </div>
                                                        </Show>
                                                        {trace.map(|trace| view! { <ExecutionTraceView trace=trace /> })}
                                                    </details>
                                                }
                                            }
                                        </For>
                                    </div>
                                </details>
                            </Show>
                        </div>
                    </Show>
                </section>
            </Show>

            <Show when=move || workspace_view.get() == WorkspaceView::Analytics && current_project.get().is_some_and(|project| project.published_version_id.is_some())>
                <section class="decision-analytics-panel" aria-label="发布版本运行分析">
                    <div class="analytics-heading">
                        <div>
                            <span class="yaml-kicker">"RUN ANALYTICS"</span>
                            <strong>"运行分析与异常发现"</strong>
                            <small>{move || decision_analytics.get().map(|report| {
                                format!("当前发布版本 v{} · 最近 {} 天 · {} 次运行", report.version_number, report.window_days, report.summary.total_runs)
                            }).unwrap_or_else(|| "正在读取当前发布版本的运行数据".into())}</small>
                        </div>
                        <div class="analytics-heading-actions">
                            <button
                                class="button button-primary"
                                on:click=generate_optimization_proposals
                                disabled=move || analytics_loading.get() || proposal_busy.get()
                            >{move || if proposal_busy.get() { "正在生成..." } else { "生成优化建议" }}</button>
                            <span class="analytics-version">{move || decision_analytics.get().map(|report| format!("v{}", report.version_number)).unwrap_or_else(|| "--".into())}</span>
                        </div>
                    </div>
                    <Show when=move || analytics_loading.get() && decision_analytics.get().is_none()>
                        <div class="analytics-state">"正在聚合运行轨迹..."</div>
                    </Show>
                    <Show when=move || analytics_error.get().is_some()>
                        <div class="analytics-error">{move || analytics_error.get().unwrap_or_default()}</div>
                    </Show>
                    {move || decision_analytics.get().map(|report| {
                        let summary = report.summary;
                        view! {
                            <div class="analytics-summary">
                                <div><strong>{summary.total_runs}</strong><span>"运行总量"</span></div>
                                <div><strong>{analytics_percent(summary.failure_rate)}</strong><span>{format!("失败 {} 次", summary.failed_runs)}</span></div>
                                <div><strong>{analytics_percent(summary.feedback_coverage_rate)}</strong><span>{format!("反馈 {} 次", summary.outcome_reported_runs)}</span></div>
                                <div class:has-alert={summary.mismatch_runs > 0}><strong>{analytics_percent(summary.mismatch_rate)}</strong><span>{format!("偏差 {} 次", summary.mismatch_runs)}</span></div>
                                <div><strong>{format!("{:.0} ms", summary.average_duration_ms)}</strong><span>"平均耗时"</span></div>
                                <div class:has-alert={summary.p95_duration_ms >= 1_000}><strong>{format!("{} ms", summary.p95_duration_ms)}</strong><span>"P95 耗时"</span></div>
                            </div>
                        }
                    })}
                    <Show when=move || decision_analytics.get().is_some_and(|report| !report.findings.is_empty())>
                        <div class="analytics-findings">
                            <strong>"确定性异常"</strong>
                            <div>
                                <For
                                    each=move || decision_analytics.get().map(|report| report.findings).unwrap_or_default()
                                    key=|finding| format!("{}:{}", finding.code, finding.element)
                                    let:finding
                                >
                                    <article class=format!("analytics-finding finding-{}", finding.level)>
                                        <div><strong>{analytics_finding_label(&finding.code)}</strong><code>{finding.code}</code></div>
                                        <span>{finding.message}</span>
                                    </article>
                                </For>
                            </div>
                        </div>
                    </Show>
                    <Show when=move || !optimization_proposals.get().is_empty() || proposal_error.get().is_some()>
                        <div class="optimization-proposals">
                            <div class="optimization-proposals-heading">
                                <div><strong>"优化建议"</strong><small>"建议基于发布版本运行快照；确认仅创建草稿，不会自动修改或发布模型"</small></div>
                                <span>{move || optimization_proposals.get().iter().filter(|proposal| proposal.status == "open").count()}</span>
                            </div>
                            <Show when=move || proposal_error.get().is_some()>
                                <div class="analytics-error">{move || proposal_error.get().unwrap_or_default()}</div>
                            </Show>
                            <div class="optimization-proposal-list">
                                <For
                                    each=move || optimization_proposals.get()
                                    key=|proposal| proposal.id
                                    let:proposal
                                >
                                    {
                                        let actions = optimization_proposal_actions(&proposal);
                                        let confirm_id = proposal.id;
                                        let dismiss_id = proposal.id;
                                        let is_open = proposal.status == "open";
                                        let resolution = if proposal.status == "confirmed" {
                                            proposal
                                                .resulting_version_id
                                                .map(|id| format!("已创建草稿 #{id}"))
                                                .unwrap_or_default()
                                        } else {
                                            "已保留人工驳回记录".into()
                                        };
                                        let resolution = RwSignal::new(resolution);
                                        view! {
                                            <article class=format!("optimization-proposal proposal-{}", proposal.finding_level)>
                                                <div class="optimization-proposal-title">
                                                    <div><strong>{proposal.title}</strong><small>{format!("v{} · {} · {}", proposal.source_version_number, proposal.finding_code, proposal.generated_at.format("%m-%d %H:%M"))}</small></div>
                                                    <span class=format!("proposal-status status-{}", proposal.status)>{optimization_proposal_status_label(&proposal.status)}</span>
                                                </div>
                                                <p>{proposal.summary}</p>
                                                <ul>
                                                    <For each=move || actions.clone() key=|action| action.clone() let:action>
                                                        <li>{action}</li>
                                                    </For>
                                                </ul>
                                                <Show when=move || is_open>
                                                    <div class="optimization-proposal-actions">
                                                        <button class="button button-primary" on:click=move |_| confirm_optimization_proposal(confirm_id) disabled=move || proposal_busy.get()>"确认并创建草稿"</button>
                                                        <button class="button button-quiet" on:click=move |_| dismiss_optimization_proposal(dismiss_id) disabled=move || proposal_busy.get()>"驳回"</button>
                                                    </div>
                                                </Show>
                                                <Show when=move || !is_open>
                                                    <small class="optimization-resolution">{move || resolution.get()}</small>
                                                </Show>
                                            </article>
                                        }
                                    }
                                </For>
                            </div>
                        </div>
                    </Show>
                    <div class="analytics-grid">
                        <div class="analytics-block">
                            <div class="analytics-block-heading"><strong>"规则覆盖"</strong><small>"按决策表统计真实命中"</small></div>
                            <Show
                                when=move || decision_analytics.get().is_some_and(|report| !report.rule_coverage.is_empty())
                                fallback=move || view! { <div class="analytics-empty">"当前发布版本没有决策表"</div> }
                            >
                                <div class="analytics-coverage-list">
                                    <For
                                        each=move || decision_analytics.get().map(|report| report.rule_coverage).unwrap_or_default()
                                        key=|metric| metric.decision_id.clone()
                                        let:metric
                                    >
                                        {
                                            let rules = metric.rules.clone();
                                            let coverage_width = format!("width: {:.1}%", metric.coverage_rate * 100.0);
                                            view! {
                                                <details class="analytics-coverage-row">
                                                    <summary>
                                                        <div><strong>{metric.decision_name}</strong><small>{format!("执行 {} 次 · 默认 {} · 无匹配 {}", metric.execution_count, metric.default_output_count, metric.no_match_count)}</small></div>
                                                        <span>{format!("{}/{}", metric.covered_rules, metric.total_rules)}</span>
                                                    </summary>
                                                    <div class="analytics-progress"><i style=coverage_width></i></div>
                                                    <div class="analytics-rule-list">
                                                        <For each=move || rules.clone() key=|rule| rule.rule_id.clone() let:rule>
                                                            <div class:uncovered={rule.hit_count == 0}>
                                                                <code>{rule.rule_id}</code>
                                                                <span>{format!("命中 {} 次", rule.hit_count)}</span>
                                                                <small>{rule.last_hit_at.map(|time| time.format("%m-%d %H:%M").to_string()).unwrap_or_else(|| "从未命中".into())}</small>
                                                            </div>
                                                        </For>
                                                    </div>
                                                </details>
                                            }
                                        }
                                    </For>
                                </div>
                            </Show>
                        </div>
                        <div class="analytics-block">
                            <div class="analytics-block-heading"><strong>"结果分布"</strong><small>"每个决策最多展示 8 个高频值"</small></div>
                            <Show
                                when=move || decision_analytics.get().is_some_and(|report| !report.result_distributions.is_empty())
                                fallback=move || view! { <div class="analytics-empty">"尚无可聚合的成功决策结果"</div> }
                            >
                                <div class="analytics-distribution-list">
                                    <For
                                        each=move || decision_analytics.get().map(|report| report.result_distributions).unwrap_or_default()
                                        key=|metric| metric.decision_id.clone()
                                        let:metric
                                    >
                                        {
                                            let values = metric.values.clone();
                                            view! {
                                                <article class="analytics-distribution">
                                                    <div><strong>{metric.decision_name}</strong><small>{format!("{} 个结果", metric.total)}</small></div>
                                                    <For each=move || values.clone() key=|value| value.value.clone() let:value>
                                                        <div class="analytics-value-row">
                                                            <code>{value.value}</code><span>{value.count}</span><small>{analytics_percent(value.rate)}</small>
                                                            <i style=format!("width: {:.1}%", value.rate * 100.0)></i>
                                                        </div>
                                                    </For>
                                                </article>
                                            }
                                        }
                                    </For>
                                </div>
                            </Show>
                        </div>
                    </div>
                    <div class="analytics-block analytics-trends">
                        <div class="analytics-block-heading"><strong>"30 日趋势"</strong><small>"运行量、失败、反馈、偏差与平均耗时"</small></div>
                        <div class="analytics-trend-list">
                            <For
                                each=move || {
                                    decision_analytics
                                        .get()
                                        .map(|report| report.daily_trends)
                                        .unwrap_or_default()
                                }
                                key=|day| day.date
                                let:day
                            >
                                {
                                    view! {
                                        <div class="analytics-trend-row">
                                            <time>{day.date.format("%m-%d").to_string()}</time>
                                            <div><i style=move || {
                                                let max_runs = decision_analytics
                                                    .get()
                                                    .and_then(|report| report.daily_trends.iter().map(|item| item.total_runs).max())
                                                    .unwrap_or(1)
                                                    .max(1);
                                                format!("width: {:.1}%", day.total_runs as f64 / max_runs as f64 * 100.0)
                                            }></i></div>
                                            <strong>{day.total_runs}</strong>
                                            <span>{format!("失败 {} · 反馈 {} · 偏差 {}", day.failed_runs, day.outcome_reported_runs, day.mismatch_runs)}</span>
                                            <small>{format!("{:.0} ms", day.average_duration_ms)}</small>
                                        </div>
                                    }
                                }
                            </For>
                        </div>
                    </div>
                </section>
            </Show>

            <Show when=move || workspace_view.get() == WorkspaceView::Validation && current_version.get().is_some() && pending_review.get().is_none()>
                <section class="business-tests-panel" aria-label="业务验收测试套件">
                    <div class="business-tests-heading">
                        <div>
                            <span class="yaml-kicker">"BUSINESS ACCEPTANCE"</span>
                            <strong>"业务验收测试套件"</strong>
                            <small>{move || {
                                let cases = decision_test_cases.get();
                                let passed = cases.iter().filter(|case| case.status == "passed").count();
                                let approved = cases.iter().filter(|case| case.approved_by.is_some()).count();
                                let selected = selected_test_case_ids.get().len();
                                format!("{} 个案例，{} 个通过，{} 个已批准 · 已选 {}", cases.len(), passed, approved, selected)
                            }}</small>
                        </div>
                        <div class="business-tests-heading-actions">
                            <button
                                class="button button-quiet"
                                on:click=select_pending_test_cases
                                disabled=move || test_case_busy.get() || !decision_test_cases.get().iter().any(|case| case.status == "passed" && case.approved_by.is_none())
                            >"选中待批准"</button>
                            <button
                                class="button button-quiet"
                                on:click=batch_approve
                                disabled=move || test_case_busy.get() || selected_test_case_ids.get().is_empty()
                            >{move || if test_case_busy.get() { "处理中..." } else { "批量批准" }}</button>
                            <button
                                class="button button-quiet"
                                on:click=batch_revoke
                                disabled=move || test_case_busy.get() || selected_test_case_ids.get().is_empty()
                            >"批量撤销"</button>
                            <button
                                class="button button-quiet"
                                on:click=generate_business_test_cases
                                disabled=move || test_case_busy.get()
                                    || current_version.get().is_some_and(|version| matches!(version.status.as_str(), "published" | "retired"))
                            >
                                {move || if test_case_running.get() { "案例执行中" } else if test_case_busy.get() { "处理中..." } else { "生成候选案例" }}
                            </button>
                            <button
                                class="button button-primary"
                                on:click=run_business_test_cases
                                disabled=move || test_case_busy.get()
                                    || decision_test_cases.get().is_empty()
                                    || current_version.get().is_some_and(|version| matches!(version.status.as_str(), "published" | "retired"))
                            >
                                {move || if test_case_running.get() { "正在运行..." } else if test_case_busy.get() { "请稍候" } else { "运行全部" }}
                            </button>
                        </div>
                    </div>
                    <div class="business-test-approval-hint">
                        <span>"仅执行通过的案例可批准；勾选案例后可批量处理。"</span>
                        <span>"批准权限受项目复核角色和版本创建者限制。"</span>
                    </div>
                    <Show when=move || test_case_running.get() || test_case_run_summary.get().is_some() || test_case_error.get().is_some()>
                        <div
                            class=move || if test_case_running.get() { "business-test-run-status is-running" } else if test_case_error.get().is_some() { "business-test-run-status is-error" } else { "business-test-run-status is-complete" }
                            role="status"
                            aria-live="polite"
                        >
                            <span class="business-test-run-indicator" aria-hidden="true"></span>
                            <div>
                                <strong>{move || if test_case_running.get() { "正在批量执行" } else if test_case_error.get().is_some() { "执行未完成" } else { "运行结果" }}</strong>
                                <span>{move || if test_case_running.get() {
                                    format!("正在执行 {} 个业务验收案例，请稍候...", decision_test_cases.get().len())
                                } else {
                                    test_case_run_summary.get().unwrap_or_default()
                                }}</span>
                            </div>
                            <Show when=move || test_case_running.get()>
                                <div class="business-test-run-progress" aria-hidden="true"><i></i></div>
                            </Show>
                        </div>
                    </Show>
                    <Show when=move || test_case_generation_summary.get().is_some()>
                        <div class="business-test-generation-summary">
                            <strong>"生成摘要"</strong>
                            <span>{move || test_case_generation_summary.get().unwrap_or_default()}</span>
                        </div>
                    </Show>
                    <Show when=move || current_version.get().is_some_and(|version| !matches!(version.status.as_str(), "published" | "retired"))>
                        <div class="business-test-form">
                            <label>
                                <span>"案例名称"</span>
                                <input
                                    type="text"
                                    maxlength="160"
                                    placeholder="例如：高风险客户拒绝贷款"
                                    prop:value=test_case_name
                                    on:input=move |event| set_test_case_name.set(event_target_value(&event))
                                />
                            </label>
                            <label>
                                <span>"输入 JSON"</span>
                                <textarea
                                    rows="3"
                                    spellcheck="false"
                                    prop:value=test_case_input
                                    on:input=move |event| set_test_case_input.set(event_target_value(&event))
                                ></textarea>
                            </label>
                            <label>
                                <span>"期望输出 JSON"</span>
                                <textarea
                                    rows="3"
                                    spellcheck="false"
                                    prop:value=test_case_expected
                                    on:input=move |event| set_test_case_expected.set(event_target_value(&event))
                                ></textarea>
                            </label>
                            <button
                                class="button button-quiet button-add-case-btn"
                                on:click=create_business_test_case
                                disabled=move || test_case_busy.get() || test_case_name.get().trim().is_empty()
                            >"添加案例"</button>
                        </div>
                    </Show>
                    <div class="business-test-list">
                        <Show
                            when=move || !decision_test_cases.get().is_empty()
                            fallback=move || view! {
                                <div class="business-test-list-empty">
                                    "暂无业务验收案例。可点击“生成候选案例”或手工添加案例。"
                                </div>
                            }
                        >
                            <For
                                each=move || decision_test_cases.get()
                                key=|test_case| format!(
                                    "{}:{}:{}:{}",
                                    test_case.id,
                                    test_case.status,
                                    test_case.updated_at,
                                    serde_json::to_string(&test_case.actual_output_json).unwrap_or_default()
                                )
                                let:test_case
                            >
                            {
                                let case_id = test_case.id;
                                let approved = test_case.approved_by.is_some();
                                let status = test_case.status.clone();
                                let status_for_class = status.clone();
                                let status_for_label = status.clone();
                                let is_generated = test_case.source == "generated";
                                let can_approve = status == "passed";
                                let can_select = can_approve;
                                let input = serde_json::to_string_pretty(&test_case.input_json).unwrap_or_default();
                                let expected = serde_json::to_string_pretty(&test_case.expected_output_json).unwrap_or_default();
                                let actual = test_case.actual_output_json
                                    .as_ref()
                                    .and_then(|value| serde_json::to_string_pretty(value).ok())
                                    .unwrap_or_else(|| "尚未运行".into());
                                let trace = test_case.execution_trace_json
                                    .and_then(|value| serde_json::from_value::<ExecutionTrace>(value).ok());
                                view! {
                                    <article
                                        class=move || format!("business-test-case case-{status_for_class}{}", if test_case_running.get() { " is-running" } else { "" })
                                        aria-busy=move || test_case_running.get()
                                    >
                                        <div class="business-test-case-heading">
                                            <div class="business-test-case-heading-title">
                                                <label class="business-test-select" title="选择此验收案例">
                                                    <input
                                                        type="checkbox"
                                                        prop:checked=move || selected_test_case_ids.get().contains(&case_id)
                                                        on:change=move |event| {
                                                            let checked = event_target_checked(&event);
                                                            set_selected_test_case_ids.update(|ids| {
                                                                if checked { ids.insert(case_id); } else { ids.remove(&case_id); }
                                                            });
                                                        }
                                                        disabled=move || test_case_busy.get() || !can_select
                                                    />
                                                </label>
                                                <strong>{test_case.name}</strong>
                                                <span>{move || if test_case_running.get() { "执行中..." } else { match status_for_label.as_str() { "passed" => "通过", "failed" => "不一致", "error" => "错误", _ => "待运行" } }}</span>
                                                <small class=if is_generated { "case-source generated" } else { "case-source manual" }>
                                                    {if is_generated { "规则生成 · 待业务核对" } else { "人工录入" }}
                                                </small>
                                            </div>
                                            <em>{if approved { "已批准" } else { "待批准" }}</em>
                                        </div>
                                        <div class="business-test-values">
                                            <div class="business-test-value value-input"><small>"输入"</small><code inner_html=move || highlight_code(&input, "json")></code></div>
                                            <div class="business-test-value value-expected"><small>"期望"</small><code inner_html=move || highlight_code(&expected, "json")></code></div>
                                            <div class="business-test-value value-actual"><small>"实际"</small><code inner_html=move || highlight_code(&actual, "json")></code></div>
                                        </div>
                                        <div class="business-test-actions">
                                            <button
                                                class="button button-quiet"
                                                on:click=move |_| approve_business_test_case(case_id, !approved)
                                                title=if !can_approve { "请先运行案例，只有执行通过的案例可以批准" } else if approved { "撤销此案例批准" } else { "批准此案例" }
                                                disabled=move || test_case_busy.get()
                                                    || !can_approve
                                                    || current_version.get().is_some_and(|version| matches!(version.status.as_str(), "published" | "retired"))
                                            >{if approved { "撤销批准" } else { "批准" }}</button>
                                            <button
                                                class="button button-quiet danger-action"
                                                on:click=move |_| delete_business_test_case(case_id)
                                                disabled=move || test_case_busy.get()
                                                    || current_version.get().is_some_and(|version| matches!(version.status.as_str(), "published" | "retired"))
                                            >"删除"</button>
                                        </div>
                                        {trace.map(|trace| view! {
                                            <details class="business-test-trace">
                                                <summary>"查看执行轨迹与解释"</summary>
                                                <ExecutionTraceView trace=trace />
                                            </details>
                                        })}
                                    </article>
                                }
                            }
                            </For>
                        </Show>
                    </div>
                    <Show when=move || test_case_error.get().is_some()>
                        <div class="business-test-error">{move || test_case_error.get().unwrap_or_default()}</div>
                    </Show>
                </section>
            </Show>

            <Show when=move || workspace_view.get() == WorkspaceView::Validation && current_version.get().is_some() && pending_review.get().is_none()>
                {move || match verification_report.get() {
                    Some(report) => {
                        let published = current_version
                            .get()
                            .is_some_and(|version| version.status == "published");
                        view! {
                            <VerificationReportView
                                report=report
                                publish_confirmed=publish_confirmed
                                set_publish_confirmed=set_publish_confirmed
                                on_verify=verify_current_version
                                on_publish=publish_current_version
                                on_review=review_current_version
                                busy=verification_busy
                                published=published
                            />
                        }.into_any()
                    }
                    None => view! {
                        <section class="verification-panel verification-empty">
                            <div class="verification-heading">
                                <div>
                                    <span class="yaml-kicker">"RELEASE GATES"</span>
                                    <strong>"尚未执行自动验收"</strong>
                                    <small>"运行基础、输入契约和阈值边界案例后才能发布"</small>
                                </div>
                                <button class="button button-primary" on:click=verify_current_version disabled=move || verification_busy.get()>
                                    {move || if verification_busy.get() { "验收中..." } else { "执行自动验收" }}
                                </button>
                            </div>
                        </section>
                    }.into_any(),
                }}
            </Show>
            <Show when=move || workspace_view.get() == WorkspaceView::Validation && release_error.get().is_some()>
                <div class="release-error">{move || release_error.get().unwrap_or_default()}</div>
            </Show>

            <Show when=move || workspace_view.get() == WorkspaceView::Model || workspace_view.get() == WorkspaceView::Discovery>
            <div class=move || if workspace_view.get() == WorkspaceView::Discovery { "model-workspace discovery-workspace" } else { "model-workspace" }>
                <Show when=move || workspace_view.get() == WorkspaceView::Discovery>
                    <section class="workflow-panel workflow-stage-panel" aria-label="需求分析与决策方案">
                        <div class="workflow-heading">
                            <div>
                                <span class="yaml-kicker">"DECISION DISCOVERY"</span>
                                <strong>{move || if decision_workflow.get().is_some() { "需求分析结果" } else { "需求分析" }}</strong>
                                <small class="workflow-heading-description">"从业务目标提炼输入、输出、规则和验收标准"</small>
                            </div>
                            <span class=move || format!("workflow-state state-{}", if workflow_busy.get() { "analyzing".to_string() } else { decision_workflow.get().map(|workflow| workflow.status).unwrap_or_else(|| "new".into()) })>
                                {move || if workflow_busy.get() { "分析中" } else { workflow_status_label(decision_workflow.get().as_ref().map(|workflow| workflow.status.as_str()).unwrap_or("new")) }}
                            </span>
                        </div>
                        <Show when=move || decision_workflow.get().is_none()>
                            <div class=move || if workflow_busy.get() { "requirement-analysis-page is-analyzing" } else { "requirement-analysis-page" } aria-busy=move || workflow_busy.get()>
                                <Show when=move || current_project.get().is_none() && workflow_error.get().is_none()>
                                    <div class="project-required-callout" role="status">
                                        <span class="project-required-callout-mark" aria-hidden="true">"!"</span>
                                        <div>
                                            <strong>"请先创建或选择决策项目"</strong>
                                            <span>"需求分析结果需要保存到项目，请先在页面顶部项目栏选择已有项目，或创建一个新项目。"</span>
                                        </div>
                                        <a class="project-required-link" href="#decision-project-selector">"前往项目栏"</a>
                                    </div>
                                </Show>
                                <div class="requirement-analysis-intro">
                                    <div class="requirement-analysis-copy">
                                        <span class="analysis-eyebrow">"START WITH THE BUSINESS QUESTION"</span>
                                        <h2>"先说清楚要解决什么问题"</h2>
                                        <p>"智能体会把自然语言需求整理成可确认的决策摘要，并识别需要补充的业务信息。确认方案后，才能生成可执行的 DMN 模型。"</p>
                                    </div>
                                    <div class="requirement-analysis-steps" aria-label="需求分析流程">
                                        <div class="analysis-step is-active"><b>"01"</b><span>"描述需求"</span><small>"说明目标、场景和限制"</small></div>
                                        <div class="analysis-step"><b>"02"</b><span>"核对分析"</span><small>"补充澄清问题和字段"</small></div>
                                        <div class="analysis-step"><b>"03"</b><span>"确认方案"</span><small>"生成模型并进入验收"</small></div>
                                    </div>
                                </div>
                                <div class="requirement-analysis-examples">
                                    <div class="analysis-section-label"><span>"可从示例开始"</span><small>"点击示例会填入输入框，可继续修改"</small></div>
                                    <div class="analysis-example-list">
                                        <button class="analysis-example" on:click=move |_| set_prompt.set("根据客户信用评分、收入和逾期次数判断贷款审批结果，并给出拒绝原因。".to_string()) disabled=move || workflow_busy.get()>
                                            <strong>"贷款审批"</strong><small>"信用评分、收入、逾期次数 → 审批结果"</small>
                                        </button>
                                        <button class="analysis-example" on:click=move |_| set_prompt.set("根据订单金额、客户等级和商品类型计算折扣，并确保折扣不超过业务上限。".to_string()) disabled=move || workflow_busy.get()>
                                            <strong>"订单折扣"</strong><small>"订单金额、客户等级 → 折扣和规则说明"</small>
                                        </button>
                                        <button class="analysis-example" on:click=move |_| set_prompt.set("根据航班延误时长、旅客舱位和是否联程，决定改签、退款或提供补偿。".to_string()) disabled=move || workflow_busy.get()>
                                            <strong>"航班服务"</strong><small>"延误时长、舱位、联程 → 服务方案"</small>
                                        </button>
                                    </div>
                                </div>
                                <Show when=move || workflow_busy.get()>
                                    <RequirementAnalysisProgress prompt=prompt agent_activity=agent_activity />
                                </Show>
                                <div class="workflow-requirement-editor analysis-editor">
                                    <label for="decision-requirement">"业务需求"</label>
                                    <textarea
                                        id="decision-requirement"
                                        class="workflow-requirement-input"
                                        placeholder="描述需要分析的业务决策需求..."
                                        prop:value=prompt
                                        on:input=move |event| set_prompt.set(event_target_value(&event))
                                        on:keydown=on_keydown
                                        disabled=move || workflow_busy.get()
                                    ></textarea>
                                    <button
                                        class="button button-primary analysis-submit"
                                        on:click=analyze_requirement
                                        disabled=move || workflow_busy.get() || prompt.get().trim().is_empty()
                                    >
                                        {move || if workflow_busy.get() { "分析中..." } else { "开始分析需求" }}
                                    </button>
                                </div>
                            </div>
                        </Show>
                        <Show when=move || decision_workflow.get().is_some()>
                            {move || decision_workflow.get().map(|workflow| view! {
                                <DecisionBriefView brief=workflow.brief />
                            })}
                        </Show>
                        <Show when=move || decision_workflow.get().as_ref().is_some_and(|workflow| !workflow.clarifications.is_empty() && workflow.plan.is_none())>
                            <div class="clarification-list">
                                <For each=move || decision_workflow.get().map(|workflow| workflow.clarifications).unwrap_or_default() key=|item| item.id.clone() let:item>
                                    <ClarificationInput item=item answers=clarification_answers set_answers=set_clarification_answers />
                                </For>
                            </div>
                        </Show>
                        <Show when=move || decision_workflow.get().and_then(|workflow| workflow.plan).is_some()>
                            {move || decision_workflow.get().and_then(|workflow| workflow.plan).map(|plan| view! { <DecisionPlanView plan=plan /> })}
                        </Show>
                        <Show when=move || decision_workflow.get().as_ref().is_some_and(|workflow| workflow.status != "confirmed")>
                            <Show when=move || workflow_analyzing.get() && decision_workflow.get().is_some()>
                                <RequirementAnalysisProgress prompt=prompt agent_activity=agent_activity />
                            </Show>
                            <div class="workflow-requirement-editor">
                                <label for="decision-requirement">"业务需求"</label>
                                <textarea
                                    id="decision-requirement"
                                    class="workflow-requirement-input"
                                    placeholder="描述需要分析的业务决策需求..."
                                    prop:value=prompt
                                    on:input=move |event| set_prompt.set(event_target_value(&event))
                                    on:keydown=on_keydown
                                    disabled=move || workflow_busy.get()
                                ></textarea>
                            </div>
                        </Show>
                        <Show when=move || workflow_error.get().is_some()>
                            <div class="workflow-error" role="alert">
                                <span class="workflow-error-icon" aria-hidden="true">"!"</span>
                                <div>
                                    <strong>{move || if workflow_error.get().is_some_and(|message| message.contains("决策项目")) { "需要先选择项目" } else { "需求分析未完成" }}</strong>
                                    <span>{move || workflow_error.get().unwrap_or_default()}</span>
                                </div>
                            </div>
                        </Show>
                        <div class="workflow-actions">
                            <Show when=move || decision_workflow.get().is_some()>
                                <button
                                    class="button button-quiet"
                                    on:click=analyze_requirement
                                    title=move || if decision_workflow.get().is_some_and(|workflow| workflow.status == "confirmed") { "方案已确认，不能重新分析" } else { "重新分析需求" }
                                    disabled=move || workflow_busy.get() || decision_workflow.get().is_none_or(|workflow| workflow.status == "confirmed" || (prompt.get().trim().is_empty() && workflow.requirement_text.trim().is_empty()))
                                >
                                    {move || if workflow_busy.get() { "分析中..." } else { "重新分析" }}
                                </button>
                            </Show>
                            <Show when=move || !workflow_analyzing.get()>
                                <Show when=move || decision_workflow.get().as_ref().is_some_and(|workflow| workflow.plan.is_none())>
                                    <button class="button button-primary" on:click=create_decision_plan disabled=move || workflow_busy.get()>
                                        {move || if workflow_busy.get() { "规划中..." } else { "生成决策方案" }}
                                    </button>
                                </Show>
                                <Show when=move || decision_workflow.get().as_ref().is_some_and(|workflow| workflow.status == "planned")>
                                    <button class="button button-primary" on:click=confirm_decision_plan disabled=move || workflow_busy.get()>
                                        {move || if workflow_busy.get() { "确认中..." } else { "确认方案" }}
                                    </button>
                                </Show>
                                <Show when=move || decision_workflow.get().as_ref().is_some_and(|workflow| workflow.status == "confirmed")>
                                    <div class="workflow-ready-actions">
                                        <span class="workflow-ready">"方案已确认，可生成模型"</span>
                                        <button
                                            class="button button-primary"
                                            on:click=generate_model_from_plan
                                            disabled=move || workflow_busy.get() || status.get() == RunStatus::Running
                                        >
                                            "使用方案生成模型"
                                        </button>
                                    </div>
                                </Show>
                            </Show>
                        </div>
                    </section>
                </Show>
            <Show when=move || workspace_view.get() == WorkspaceView::Model>
            <section class="workspace-grid">
                <section class="conversation-panel panel">
                    <div class="panel-heading conversation-heading">
                        <div class="heading-copy">
                            <p class="eyebrow">"AGENT CONVERSATION"</p>
                            <h2>{move || if generation_mode.get() == GenerationMode::Modify { "修改当前决策模型" } else { "方案驱动建模" }}</h2>
                            <p class="heading-description">{move || if generation_mode.get() == GenerationMode::Modify { "描述需要调整的规则，智能体将提交受控增量修改。" } else { "请在需求与方案页确认方案，再使用方案生成模型。" }}</p>
                        </div>
                    </div>
                    <div class="conversation-content">
                    <div class="conversation-scroll" node_ref=message_list>
                    <div class="message-list" aria-live="polite">
                        <For each=move || messages.get() key=|message| message.content.clone() let:message>
                            <article class=format!("message message-{}", message.role)>
                                <span class="message-label">{if message.role == "user" { "你" } else { "智能体" }}</span>
                                <p>{message.content}</p>
                            </article>
                        </For>
                        <Show when=move || !assistant_delta.get().is_empty()>
                            <article class=move || if status.get() == RunStatus::Succeeded { "message message-assistant message-live message-dsl-preview" } else { "message message-assistant message-live" }>
                                <span class="message-label">{move || if status.get() == RunStatus::Succeeded { "已验证 DSL 预览" } else { "智能体正在生成" }}</span>
                                <p>{move || assistant_delta.get()}</p>
                            </article>
                        </Show>
                        <Show when=move || status.get() == RunStatus::Running && assistant_delta.get().is_empty()>
                            <article class="message message-assistant message-pending" role="status" aria-live="polite">
                                <span class="message-label">"智能体"</span>
                                <p>{move || agent_activity.get()}</p>
                            </article>
                        </Show>
                    </div>
                    <Show when=move || pending_review.get().is_some()>
                        {move || pending_review.get().map(|review| view! {
                            <VersionChangeReviewView
                                review=review
                                confirmed=review_confirmed
                                set_confirmed=set_review_confirmed
                                on_confirm=confirm_change
                                on_discard=discard_change
                                saving=version_saving
                            />
                        })}
                    </Show>
                    </div>
                    <div class="composer-wrap">
                        <Show when=move || pending_review.get().is_some()>
                            <div class="model-change-lock" role="status">
                                <strong>"当前 DSL 已生成，等待变更确认"</strong>
                                <span>"请在上方变更确认面板勾选确认并保存为草稿版本，保存完成后即可修改当前模型。"</span>
                            </div>
                        </Show>
                        <Show when=move || generation_mode.get() == GenerationMode::Create>
                            <div class="model-plan-guidance">"新增模型请返回“需求与方案”，确认方案后点击“使用方案生成模型”。"</div>
                        </Show>
                        <Show when=move || generation_mode.get() == GenerationMode::Modify>
                            <label class="known-nl_dsl-label" for="known-nl_dsl">"当前版本 DSL · 受控补丁基线"</label>
                            <textarea
                                id="known-nl_dsl"
                                class="known-nl_dsl-editor"
                                prop:value=source_dsl
                                readonly=true
                            ></textarea>
                        </Show>
                        <Show when=move || generation_mode.get() == GenerationMode::Modify>
                            <textarea
                                class="composer"
                                placeholder="描述要应用到 DSL 的修改..."
                                prop:value=prompt
                                on:input=move |event| set_prompt.set(event_target_value(&event))
                                on:keydown=on_keydown
                                disabled=move || status.get() == RunStatus::Running || pending_review.get().is_some()
                            ></textarea>
                        </Show>
                        <div class="composer-footer">
                            <span class="composer-hint">
                                {move || if generation_mode.get() == GenerationMode::Create {
                                    match decision_workflow.get().as_ref().map(|workflow| workflow.status.as_str()) {
                                        None => "Ctrl / Cmd + Enter 分析需求",
                                        Some("analyzed" | "clarification") => "Ctrl / Cmd + Enter 生成方案",
                                        Some("planned") => "Ctrl / Cmd + Enter 确认方案",
                                        _ => "Ctrl / Cmd + Enter 生成模型",
                                    }
                                } else {
                                    "Ctrl / Cmd + Enter 发送"
                                }}
                            </span>
                            <div class="composer-actions">
                                <Show when=move || status.get() == RunStatus::Running>
                                    <button class="button button-danger" on:click=stop>"停止运行"</button>
                                </Show>
                                <Show when=move || generation_mode.get() == GenerationMode::Modify>
                                <button
                                    class="button button-primary"
                                    on:click=move |_| submit()
                                    disabled=move || prompt.get().trim().is_empty()
                                        || status.get() == RunStatus::Running
                                        || version_saving.get()
                                    || current_project.get().is_none()
                                    || current_version.get().is_none()
                                    || pending_review.get().is_some()
                                        || (generation_mode.get() == GenerationMode::Create
                                            && !decision_workflow.get().as_ref().is_some_and(|workflow| workflow.status == "confirmed"))
                                        || (generation_mode.get() == GenerationMode::Modify && source_dsl.get().trim().is_empty())
                                >{move || if status.get() == RunStatus::Running { "处理中..." } else { "提交修改" }}</button>
                                </Show>
                            </div>
                        </div>
                    </div>
                    </div>
                </section>

                <aside class="inspector-column">
                    <section class="panel timeline-panel">
                        <div class="panel-heading compact-heading">
                            <div>
                                <p class="eyebrow">"RUN TIMELINE"</p>
                                <h2>"执行过程"</h2>
                            </div>
                            <span class="step-count">{move || steps.get().len()}" steps"</span>
                        </div>
                        <div class="timeline">
                            <div class=move || format!("agent-activity {}", status.get().class()) role="status" aria-live="polite">
                                <span class="agent-activity-indicator" aria-hidden="true"></span>
                                <span>{move || agent_activity.get()}</span>
                            </div>
                            <Show when=move || steps.get().is_empty()>
                                <p class="muted-copy">"发送需求后，这里会显示生成、校验和编译进度。"</p>
                            </Show>
                            <For each=move || timeline_entries(steps.get()) key=|(index, _)| *index let:entry>
                                {let (index, step) = entry;
                                let text = timeline_item_text(&step);
                                let class_step = step.clone();
                                view! {
                                    <div class=move || {
                                        let is_current = index + 1 == steps.get().len();
                                        format!("timeline-item {}", timeline_item_class(&class_step, is_current))
                                    }>
                                        <span class="timeline-marker" aria-hidden="true"></span>
                                        <span>{text}</span>
                                    </div>
                                }}
                            </For>
                        </div>
                    </section>

                    <section class="panel artifacts-panel">
                        <div class="panel-heading compact-heading">
                            <div>
                                <p class="eyebrow">"ARTIFACTS"</p>
                                <h2>"模型产物"</h2>
                            </div>
                            <Show when=move || dsl.get().is_some()>
                                <span class="artifact-ready">"READY"</span>
                            </Show>
                        </div>
                        <ArtifactTabs
                            dsl=dsl
                            set_dsl=set_dsl
                            xml=xml
                            set_xml=set_xml
                            set_source=set_source_dsl
                        />
                    </section>
                </aside>
            </section>
            </Show>
            </div>
            </Show>

            <Show when=move || show_test.get()>
                <div class="test-drawer-backdrop" on:click=move |_| set_show_test.set(false)></div>
                <aside class="test-drawer" aria-label="DMN 核验与测试">
                    <div class="test-drawer-heading">
                        <div class="test-title-block">
                            <span class="test-title-icon">"D"</span>
                            <div><p class="eyebrow">"DMN QUALITY & EVALUATION"</p><h2>"核验与测试"</h2></div>
                        </div>
                        <button class="button button-quiet test-close" on:click=move |_| set_show_test.set(false) title="关闭核验与测试面板">"关闭"</button>
                    </div>
                    <div class="test-body" node_ref=test_drawer_body>
                        <section class="test-workbench">
                            <div class="test-model-editor">
                                <div class="test-section-label"><span>"01"</span><strong>"DMN 内容"</strong></div>
                                <pre class="code-view code-view-xml test-xml-preview" tabindex="0" aria-label="当前 DMN XML 源码"><code inner_html=move || {
                                    let value = test_xml.get();
                                    let source = if value.trim().is_empty() { "暂无可展示的 DMN XML" } else { value.as_str() };
                                    highlight_code(source, "xml")
                                }></code></pre>
                                <div class="test-actions test-xml-actions">
                                    <button class="button button-quiet" on:click=move |_| { if let Some(value) = xml.get() { set_test_xml.set(value); } } disabled=move || xml.get().is_none()>"刷新当前 XML"</button>
                                    <button class="button button-primary" on:click=move |_| verify_test_model() disabled=move || analysis_running.get()>{move || if analysis_running.get() { "核验中..." } else { "核验模型" }}</button>
                                </div>
                            </div>
                            <section class=move || format!("validation-card {}", if validation_result.get().is_some() { "is-valid" } else if validation_error.get().is_some() { "is-invalid" } else { "is-idle" })>
                                <div class="test-section-label"><span>"02"</span><strong>"模型状态"</strong></div>
                                <div class="validation-card-heading">
                                    <span class="validation-icon">{move || if validation_running.get() { "..." } else if validation_result.get().is_some() { "OK" } else if validation_error.get().is_some() { "!" } else { "--" }}</span>
                                    <div><strong>{move || if validation_running.get() { "正在核验模型" } else if validation_result.get().is_some() { "模型核验通过" } else if validation_error.get().is_some() { "模型核验失败" } else { "尚未核验" }}</strong><small>{move || validation_result.get().and_then(|result| result.summary).map(|summary| format!("{} 个决策 · {} 个输入 · {} 个数据类型", summary["decisions"].as_u64().unwrap_or_default(), summary["inputs"].as_u64().unwrap_or_default(), summary["itemDefinitions"].as_u64().unwrap_or_default())).or_else(|| validation_error.get()).unwrap_or_else(|| "核验后将自动填充测试输入".to_string())}</small></div>
                                </div>
                            </section>
                        </section>
                        <div class="test-section-label test-section-input"><span>"03"</span><strong>"测试输入"</strong></div>
                        <label class="test-input-label" for="dmn-test-input">"JSON 输入"</label>
                        <p class="test-input-hint">"JSON 键使用上方的执行键；模型定义 variable 时，执行键与 inputData 显示名称可能不同。"</p>
                        <textarea id="dmn-test-input" class="test-input" prop:value=test_input on:input=move |event| set_test_input.set(event_target_value(&event)) placeholder="{\n  \"Age\": 35\n}"></textarea>
                        <div class="test-actions">
                            <button class="button button-primary" on:click=run_test disabled=move || test_running.get() || test_xml.get().trim().is_empty()>
                                {move || if test_running.get() { "执行中..." } else { "执行测试" }}
                            </button>
                            <button class="button button-quiet" on:click=move |_| set_test_input.set("{}".to_string())>"清空输入"</button>
                        </div>
                        <div class="test-section-label test-section-results"><span>"04"</span><strong>"执行结果"</strong></div>
                        <Show when=move || test_error.get().is_some()>
                            <div class="test-feedback error-msg" role="alert">{move || test_error.get().unwrap_or_default()}</div>
                        </Show>
                        <Show when=move || test_result.get().is_some()>
                            <div class="test-results" tabindex="0" aria-live="polite">
                                <div class="test-result-summary"><span class="result-summary-label">"执行结果"</span>{move || test_result.get().map(|result| format!("{} · {} 个决策 · {} 条诊断", result.status.unwrap_or_default(), result.decisions.len(), result.messages.len())).unwrap_or_default()}</div>
                                <div class="test-decision-results">
                                    <For
                                        each=move || test_result.get().map(|result| result.decisions).unwrap_or_default()
                                        key=|decision| decision.id.clone()
                                        let:decision
                                    >
                                        <article class="test-decision-result">
                                            <div class="test-decision-result-heading">
                                                <strong>{decision.name}</strong>
                                                <span>{decision.status}</span>
                                            </div>
                                            <code>{decision.value}</code>
                                            {if decision.matched_rule_ids.is_empty() {
                                                None
                                            } else {
                                                Some(view! { <small>{format!("命中规则：{}", decision.matched_rule_ids.join("、"))}</small> })
                                            }}
                                            {if decision.evaluated_dependencies.is_empty() {
                                                None
                                            } else {
                                                Some(view! { <small>{format!("已计算依赖：{}", decision.evaluated_dependencies.join("、"))}</small> })
                                            }}
                                        </article>
                                    </For>
                                </div>
                                {move || test_result.get().and_then(|result| result.trace).map(|trace| view! { <ExecutionTraceView trace=trace /> })}
                            </div>
                        </Show>
                    </div>
                </aside>
            </Show>

            <Show when=move || error.get().is_some()>
                <div class="error-banner" role="alert">
                    <strong>"运行未完成"</strong>
                    <span>{move || error.get().unwrap_or_default()}</span>
                    <button class="button button-quiet" on:click=retry disabled=move || status.get() == RunStatus::Running>
                        "重试上次请求"
                    </button>
                </div>
            </Show>
            <footer class="footer-note">"同源安全连接 · API 密钥仅保留在服务端"</footer>
        </main>
    }
}

fn apply_result(
    result: RunResult,
    set_dsl: WriteSignal<Option<String>>,
    set_xml: WriteSignal<Option<String>>,
    set_test_xml: WriteSignal<String>,
    set_test_result: WriteSignal<Option<EvaluateResult>>,
    set_test_error: WriteSignal<Option<String>>,
    set_validation_result: WriteSignal<Option<api::ValidateDmnResult>>,
    set_validation_error: WriteSignal<Option<String>>,
    set_error: WriteSignal<Option<String>>,
    set_messages: WriteSignal<Vec<ChatMessage>>,
    set_status: WriteSignal<RunStatus>,
    set_generation_mode: WriteSignal<GenerationMode>,
    set_source_dsl: WriteSignal<String>,
    set_workspace_view: WriteSignal<WorkspaceView>,
) {
    if result.ok {
        let (Some(dsl), Some(xml)) = (result.dsl.clone(), result.xml.clone()) else {
            set_error.set(Some(
                "生成结果不完整，未返回 DSL 或 DMN XML，模型产物未更新".into(),
            ));
            set_status.set(RunStatus::Failed);
            return;
        };

        // Keep every consumer on the generated model snapshot. Previously the
        // test drawer retained its old XML until the user pressed refresh.
        set_dsl.set(Some(dsl.clone()));
        set_xml.set(Some(xml.clone()));
        set_source_dsl.set(dsl.clone());
        set_test_xml.set(xml);
        set_test_result.set(None);
        set_test_error.set(None);
        set_validation_result.set(None);
        set_validation_error.set(None);
        set_workspace_view.set(WorkspaceView::Model);

        let message = result
            .change_summary
            .as_ref()
            .map(|summary| incremental_result_message(summary, dsl.len()))
            .unwrap_or_else(|| format!("模型已生成并通过校验。DSL 共 {} 个字符。", dsl.len()));
        set_messages.update(|items| {
            items.push(ChatMessage {
                role: "assistant",
                content: message,
            })
        });
        set_status.set(RunStatus::Succeeded);
        set_generation_mode.set(GenerationMode::Modify);
    } else {
        set_error.set(
            result
                .error
                .or_else(|| Some("智能体未返回有效结果".to_string())),
        );
        set_status.set(RunStatus::Failed);
    }
}

fn incremental_result_message(summary: &ChangeSummary, dsl_length: usize) -> String {
    let mut changes = Vec::new();
    if !summary.added.is_empty() {
        changes.push(format!("新增 {}", summary_names(&summary.added)));
    }
    if !summary.updated.is_empty() {
        changes.push(format!("更新 {}", summary_names(&summary.updated)));
    }
    if !summary.removed.is_empty() {
        changes.push(format!("删除 {}", summary_names(&summary.removed)));
    }

    if changes.is_empty() {
        format!(
            "增量修改已完成，未检测到命名 DMN 元素变更。DSL 共 {} 个字符。",
            dsl_length
        )
    } else {
        format!(
            "增量修改已完成：{}。DSL 共 {} 个字符。",
            changes.join("；"),
            dsl_length
        )
    }
}

fn summary_names(names: &[String]) -> String {
    const MAX_VISIBLE_NAMES: usize = 8;
    let visible = names
        .iter()
        .take(MAX_VISIBLE_NAMES)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("、");
    if names.len() > MAX_VISIBLE_NAMES {
        format!("{visible} 等 {} 项", names.len())
    } else {
        visible
    }
}

#[component]
fn Artifact(
    label: &'static str,
    language: &'static str,
    value: ReadSignal<Option<String>>,
    set_value: Option<WriteSignal<Option<String>>>,
    set_xml: Option<WriteSignal<Option<String>>>,
    set_source: Option<WriteSignal<String>>,
) -> impl IntoView {
    if language == "yaml" {
        return view! { <YamlArtifact value=value set_value=set_value set_xml=set_xml set_source=set_source /> }
            .into_any();
    }

    let copy_status = RwSignal::new(CopyStatus::Idle);
    Effect::new(move |_| {
        let _ = value.get();
        copy_status.set(CopyStatus::Idle);
    });
    let copy = move |_| {
        let Some(content) = value.get() else {
            return;
        };
        copy_status.set(CopyStatus::Idle);
        spawn_local(async move {
            let result = if let Some(window) = web_sys::window() {
                JsFuture::from(window.navigator().clipboard().write_text(&content))
                    .await
                    .map_err(|_| ())
            } else {
                Err(())
            };
            copy_status.set(if result.is_ok() {
                CopyStatus::Copied
            } else {
                CopyStatus::Failed
            });
        });
    };

    view! {
        <div class="artifact-block">
            <div class="artifact-heading">
                <span>{label}</span>
                <div class="artifact-heading-actions">
                    <span class="artifact-type">{language.to_uppercase()}</span>
                    <button
                        class="button button-quiet artifact-copy"
                        title=format!("复制 {label} 内容")
                        on:click=copy
                        disabled=move || value.get().is_none()
                    >
                        {move || match copy_status.get() {
                            CopyStatus::Idle => "复制",
                            CopyStatus::Copied => "已复制",
                            CopyStatus::Failed => "复制失败",
                        }}
                    </button>
                </div>
            </div>
            <pre class=format!("code-view code-view-{}", language)>
                <code inner_html=move || highlight_code(
                    &value.get().unwrap_or_else(|| "等待生成...".to_string()),
                    language,
                )></code>
            </pre>
        </div>
    }
    .into_any()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ArtifactTab {
    Yaml,
    Xml,
}

#[component]
fn ArtifactTabs(
    dsl: ReadSignal<Option<String>>,
    set_dsl: WriteSignal<Option<String>>,
    xml: ReadSignal<Option<String>>,
    set_xml: WriteSignal<Option<String>>,
    set_source: WriteSignal<String>,
) -> impl IntoView {
    let active = RwSignal::new(ArtifactTab::Yaml);
    view! {
        <div class="artifact-workbench">
            <div class="artifact-tabs" role="tablist" aria-label="模型产物">
                <button
                    class=move || if active.get() == ArtifactTab::Yaml { "artifact-tab is-active" } else { "artifact-tab" }
                    on:click=move |_| active.set(ArtifactTab::Yaml)
                    role="tab"
                    aria-selected=move || active.get() == ArtifactTab::Yaml
                >"DSL YAML"</button>
                <button
                    class=move || if active.get() == ArtifactTab::Xml { "artifact-tab is-active" } else { "artifact-tab" }
                    on:click=move |_| active.set(ArtifactTab::Xml)
                    role="tab"
                    aria-selected=move || active.get() == ArtifactTab::Xml
                >"DMN XML"</button>
            </div>
            <div class="artifact-tab-content">
                <Show when=move || active.get() == ArtifactTab::Yaml>
                    <YamlArtifact value=dsl set_value=Some(set_dsl) set_xml=Some(set_xml) set_source=Some(set_source) />
                </Show>
                <Show when=move || active.get() == ArtifactTab::Xml>
                    <Artifact label="DMN XML" language="xml" value=xml set_value=None set_xml=None set_source=None />
                </Show>
            </div>
        </div>
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum YamlView {
    Model,
    Dependency,
    Structure,
    Source,
}

#[derive(Clone, PartialEq, Eq, Default)]
struct YamlSummary {
    name: String,
    namespace: String,
    description: String,
    item_definitions: usize,
    inputs: usize,
    input_details: Vec<YamlField>,
    type_details: Vec<YamlField>,
    decisions: Vec<YamlDecision>,
    graph_nodes: Vec<YamlGraphNode>,
    graph_edges: Vec<YamlGraphEdge>,
    bkms: Vec<YamlKnowledgeModel>,
    decision_services: Vec<YamlDecisionService>,
    knowledge_sources: usize,
    text_annotations: usize,
    raw: Option<YamlValue>,
    error: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Default)]
struct YamlDecision {
    name: String,
    element_type: String,
    description: String,
    type_ref: String,
    hit_policy: String,
    inputs: Vec<String>,
    outputs: Vec<String>,
    rules: Vec<YamlRule>,
    required_inputs: Vec<String>,
    required_decisions: Vec<String>,
    required_knowledge: Vec<String>,
    expression_type: String,
    expression_summary: String,
}

#[derive(Clone, PartialEq, Eq, Default)]
struct YamlKnowledgeModel {
    name: String,
    element_type: String,
    description: String,
    type_ref: String,
    parameters: Vec<String>,
    expression_type: String,
    expression_summary: String,
}

#[derive(Clone, PartialEq, Eq, Default)]
struct YamlDecisionService {
    name: String,
    element_type: String,
    description: String,
    input_data: Vec<String>,
    encapsulated_decisions: Vec<String>,
    output_decisions: Vec<String>,
    expression_summary: String,
}

#[derive(Clone, PartialEq, Eq, Default)]
struct YamlGraphNode {
    name: String,
    kind: String,
    x: i32,
    y: i32,
}

#[derive(Clone, PartialEq, Eq, Default)]
struct YamlGraphEdge {
    source: String,
    target: String,
    kind: String,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
}

const YAML_GRAPH_WIDTH: i32 = 800;
const YAML_GRAPH_NODE_WIDTH: i32 = 156;
const YAML_GRAPH_NODE_HEIGHT: i32 = 50;

fn yaml_graph_height(nodes: &[YamlGraphNode]) -> i32 {
    nodes
        .iter()
        .map(|node| node.y + YAML_GRAPH_NODE_HEIGHT + 34)
        .max()
        .unwrap_or(260)
        .max(260)
}

fn yaml_graph_edge_anchors(source: &YamlGraphNode, target: &YamlGraphNode) -> (i32, i32, i32, i32) {
    if source.x < target.x {
        (
            source.x + YAML_GRAPH_NODE_WIDTH,
            source.y + YAML_GRAPH_NODE_HEIGHT / 2,
            target.x,
            target.y + YAML_GRAPH_NODE_HEIGHT / 2,
        )
    } else if source.x > target.x {
        (
            source.x,
            source.y + YAML_GRAPH_NODE_HEIGHT / 2,
            target.x + YAML_GRAPH_NODE_WIDTH,
            target.y + YAML_GRAPH_NODE_HEIGHT / 2,
        )
    } else if source.y <= target.y {
        (
            source.x + YAML_GRAPH_NODE_WIDTH / 2,
            source.y + YAML_GRAPH_NODE_HEIGHT,
            target.x + YAML_GRAPH_NODE_WIDTH / 2,
            target.y,
        )
    } else {
        (
            source.x + YAML_GRAPH_NODE_WIDTH / 2,
            source.y,
            target.x + YAML_GRAPH_NODE_WIDTH / 2,
            target.y + YAML_GRAPH_NODE_HEIGHT,
        )
    }
}

fn yaml_graph_edge_path(edge: &YamlGraphEdge) -> String {
    if edge.x1 == edge.x2 {
        let control = (edge.y2 - edge.y1) as f32 * 0.45;
        format!(
            "M {} {} C {} {:.1}, {} {:.1}, {} {}",
            edge.x1,
            edge.y1,
            edge.x1,
            edge.y1 as f32 + control,
            edge.x2,
            edge.y2 as f32 - control,
            edge.x2,
            edge.y2
        )
    } else {
        let control = (edge.x2 - edge.x1) as f32 * 0.45;
        format!(
            "M {} {} C {:.1} {}, {:.1} {}, {} {}",
            edge.x1,
            edge.y1,
            edge.x1 as f32 + control,
            edge.y1,
            edge.x2 as f32 - control,
            edge.y2,
            edge.x2,
            edge.y2
        )
    }
}

fn yaml_graph_label(name: &str) -> String {
    const MAX_CHARS: usize = 18;
    let mut chars = name.chars();
    let label = chars.by_ref().take(MAX_CHARS).collect::<String>();
    if chars.next().is_some() {
        format!("{label}...")
    } else {
        label
    }
}

fn yaml_graph_knowledge_points(node: &YamlGraphNode) -> String {
    let x = node.x;
    let y = node.y;
    let right = x + YAML_GRAPH_NODE_WIDTH;
    let bottom = y + YAML_GRAPH_NODE_HEIGHT;
    format!(
        "{},{} {},{} {},{} {},{} {},{} {},{}",
        x + 16,
        y,
        x,
        y + 16,
        x,
        bottom,
        right - 16,
        bottom,
        right,
        bottom - 16,
        right,
        y
    )
}

#[derive(Clone, PartialEq, Eq, Default)]
struct YamlRule {
    inputs: Vec<String>,
    outputs: Vec<String>,
    annotation: String,
}

#[derive(Clone, PartialEq, Eq, Default)]
struct YamlField {
    name: String,
    element_type: String,
    type_ref: String,
    description: String,
}

fn yaml_map(value: &YamlValue) -> Option<&serde_yaml::Mapping> {
    match value {
        YamlValue::Mapping(map) => Some(map),
        _ => None,
    }
}

fn yaml_get<'a>(map: &'a serde_yaml::Mapping, key: &str) -> Option<&'a YamlValue> {
    map.get(YamlValue::String(key.to_string()))
}

fn yaml_text(value: Option<&YamlValue>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    match value {
        YamlValue::Null => String::new(),
        YamlValue::String(text) => text.clone(),
        _ => serde_yaml::to_string(value)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}

fn yaml_list(value: Option<&YamlValue>) -> Vec<YamlValue> {
    match value {
        Some(YamlValue::Sequence(items)) => items.clone(),
        _ => Vec::new(),
    }
}

fn yaml_joined_list(value: Option<&YamlValue>) -> String {
    yaml_list(value)
        .iter()
        .map(|item| yaml_text(Some(item)))
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Builds a compact, readable preview for every boxed-expression variant.
/// Invocation and function expressions do not have a top-level `expression`
/// field, so falling back to a generic placeholder made those definitions
/// appear empty in the model panel.
fn yaml_expression_preview(logic: &serde_yaml::Mapping) -> String {
    let expression_type = yaml_text(yaml_get(logic, "type"));
    match expression_type.as_str() {
        "literalExpression" => ["expression", "text", "value", "body", "literal"]
            .iter()
            .find_map(|key| yaml_get(logic, key).map(|value| yaml_text(Some(value))))
            .unwrap_or_default(),
        "invocation" => {
            let bkm = yaml_text(yaml_get(logic, "bkm"));
            let bindings = yaml_list(yaml_get(logic, "bindings"))
                .iter()
                .filter_map(|value| {
                    let binding = yaml_map(value)?;
                    let parameter = yaml_text(yaml_get(binding, "parameter"));
                    let expression = yaml_text(yaml_get(binding, "expression"));
                    Some(if parameter.is_empty() {
                        expression
                    } else if expression.is_empty() {
                        parameter
                    } else {
                        format!("{parameter} = {expression}")
                    })
                })
                .filter(|binding| !binding.is_empty())
                .collect::<Vec<_>>();
            if bindings.is_empty() {
                format!("{bkm}()")
            } else {
                format!("{bkm}({})", bindings.join(", "))
            }
        }
        "functionDefinition" => {
            let parameters = yaml_list(yaml_get(logic, "parameters"))
                .iter()
                .filter_map(|value| {
                    let parameter = yaml_map(value)?;
                    let name = yaml_text(yaml_get(parameter, "name"));
                    let type_ref = yaml_text(yaml_get(parameter, "typeRef"));
                    Some(if type_ref.is_empty() {
                        name
                    } else {
                        format!("{name}: {type_ref}")
                    })
                })
                .filter(|parameter| !parameter.is_empty())
                .collect::<Vec<_>>();
            let body = yaml_get(logic, "body")
                .and_then(yaml_map)
                .map(yaml_expression_preview)
                .unwrap_or_default();
            format!(
                "function ({}){}",
                parameters.join(", "),
                if body.is_empty() {
                    String::new()
                } else {
                    format!(" {{ {body} }}")
                }
            )
        }
        "context" => {
            let entries = yaml_list(yaml_get(logic, "entries"))
                .iter()
                .filter_map(|value| {
                    let entry = yaml_map(value)?;
                    let key = yaml_text(yaml_get(entry, "key"));
                    let value = yaml_get(entry, "value")
                        .and_then(yaml_map)
                        .map(yaml_expression_preview)
                        .unwrap_or_else(|| yaml_text(yaml_get(entry, "value")));
                    Some(if value.is_empty() {
                        key
                    } else {
                        format!("{key}: {value}")
                    })
                })
                .filter(|entry| !entry.is_empty())
                .collect::<Vec<_>>();
            format!("{{ {} }}", entries.join(", "))
        }
        "list" => format!("[{}]", yaml_joined_list(yaml_get(logic, "items"))),
        "relation" => {
            let columns = yaml_joined_list(yaml_get(logic, "columns"));
            let rows = yaml_list(yaml_get(logic, "rows")).len();
            format!("relation ({columns}) · {rows} 行")
        }
        "conditional" => {
            let condition = yaml_text(yaml_get(logic, "condition"));
            let then = yaml_get(logic, "then")
                .and_then(yaml_map)
                .map(yaml_expression_preview)
                .unwrap_or_default();
            let else_ = yaml_get(logic, "else")
                .and_then(yaml_map)
                .map(yaml_expression_preview)
                .unwrap_or_default();
            format!("if {condition} then {then} else {else_}")
        }
        "filter" => format!(
            "{}[{}]",
            yaml_text(yaml_get(logic, "source")),
            yaml_text(yaml_get(logic, "filter"))
        ),
        "for" => format!(
            "for {} in {} return {}",
            yaml_text(yaml_get(logic, "variable")),
            yaml_text(yaml_get(logic, "in")),
            yaml_text(yaml_get(logic, "return"))
        ),
        "quantified" => format!(
            "{} {} in {} satisfies {}",
            yaml_text(yaml_get(logic, "kind")),
            yaml_text(yaml_get(logic, "variable")),
            yaml_text(yaml_get(logic, "in")),
            yaml_text(yaml_get(logic, "satisfies"))
        ),
        "decisionTable" => {
            let inputs = yaml_list(yaml_get(logic, "inputs")).len();
            let outputs = yaml_list(yaml_get(logic, "outputs")).len();
            let rules = yaml_list(yaml_get(logic, "rules")).len();
            format!("决策表 · {inputs} 个输入 · {outputs} 个输出 · {rules} 条规则")
        }
        _ => serde_yaml::to_string(logic)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}

fn yaml_summary(source: &str) -> YamlSummary {
    let raw = match serde_yaml::from_str::<YamlValue>(source) {
        Ok(value) => value,
        Err(error) => {
            return YamlSummary {
                error: Some(format!("YAML 解析失败：{error}")),
                ..Default::default()
            };
        }
    };
    let Some(root) = yaml_map(&raw) else {
        return YamlSummary {
            raw: Some(raw),
            error: Some("YAML 根节点必须是对象".into()),
            ..Default::default()
        };
    };
    let decisions = yaml_list(yaml_get(root, "decisions"))
        .iter()
        .filter_map(|value| {
            let map = yaml_map(value)?;
            let logic = yaml_get(map, "logic").and_then(yaml_map);
            let table = logic.filter(|logic| yaml_text(yaml_get(logic, "type")) == "decisionTable");
            let expression_type = logic
                .map(|logic| yaml_text(yaml_get(logic, "type")))
                .unwrap_or_else(|| "literalExpression".into());
            let expression_summary = logic.map(yaml_expression_preview).unwrap_or_default();
            let inputs = table
                .and_then(|table| yaml_get(table, "inputs"))
                .map(|value| yaml_list(Some(value)))
                .unwrap_or_default()
                .iter()
                .map(|value| {
                    yaml_map(value)
                        .map(|input| yaml_text(yaml_get(input, "name")))
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>();
            let outputs = table
                .and_then(|table| yaml_get(table, "outputs"))
                .map(|value| yaml_list(Some(value)))
                .unwrap_or_default()
                .iter()
                .map(|value| {
                    yaml_map(value)
                        .map(|output| yaml_text(yaml_get(output, "name")))
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>();
            let rules = table
                .and_then(|table| yaml_get(table, "rules"))
                .map(|value| yaml_list(Some(value)))
                .unwrap_or_default()
                .iter()
                .filter_map(|value| {
                    let rule = yaml_map(value)?;
                    Some(YamlRule {
                        inputs: yaml_list(yaml_get(rule, "inputEntries"))
                            .iter()
                            .map(|v| yaml_text(Some(v)))
                            .collect(),
                        outputs: yaml_list(yaml_get(rule, "outputEntries"))
                            .iter()
                            .map(|v| yaml_text(Some(v)))
                            .collect(),
                        annotation: yaml_text(yaml_get(rule, "annotation")),
                    })
                })
                .collect();
            Some(YamlDecision {
                name: yaml_text(yaml_get(map, "name")),
                element_type: "决策".into(),
                description: yaml_text(yaml_get(map, "description")),
                type_ref: yaml_text(yaml_get(map, "typeRef")),
                hit_policy: table
                    .map(|table| yaml_text(yaml_get(table, "hitPolicy")))
                    .unwrap_or_default(),
                inputs,
                outputs,
                rules,
                required_inputs: yaml_list(yaml_get(map, "requiredInputs"))
                    .iter()
                    .map(|value| yaml_text(Some(value)))
                    .collect(),
                required_decisions: yaml_list(yaml_get(map, "requiredDecisions"))
                    .iter()
                    .map(|value| yaml_text(Some(value)))
                    .collect(),
                required_knowledge: yaml_list(yaml_get(map, "requiredKnowledge"))
                    .iter()
                    .map(|value| yaml_text(Some(value)))
                    .collect(),
                expression_type,
                expression_summary,
            })
        })
        .collect::<Vec<_>>();
    let bkms = yaml_list(yaml_get(root, "bkms"))
        .iter()
        .filter_map(|value| {
            let map = yaml_map(value)?;
            let logic = yaml_get(map, "logic").and_then(yaml_map);
            Some(YamlKnowledgeModel {
                name: yaml_text(yaml_get(map, "name")),
                element_type: "业务知识模型".into(),
                description: yaml_text(yaml_get(map, "description")),
                type_ref: yaml_text(yaml_get(map, "typeRef")),
                parameters: yaml_list(yaml_get(map, "parameters"))
                    .iter()
                    .filter_map(|value| {
                        let parameter = yaml_map(value)?;
                        let name = yaml_text(yaml_get(parameter, "name"));
                        let type_ref = yaml_text(yaml_get(parameter, "typeRef"));
                        Some(if type_ref.is_empty() {
                            name
                        } else {
                            format!("{name}: {type_ref}")
                        })
                    })
                    .filter(|parameter| !parameter.is_empty())
                    .collect(),
                expression_type: logic
                    .map(|logic| yaml_text(yaml_get(logic, "type")))
                    .unwrap_or_else(|| "literalExpression".into()),
                expression_summary: logic.map(yaml_expression_preview).unwrap_or_default(),
            })
        })
        .filter(|bkm| !bkm.name.is_empty())
        .collect::<Vec<_>>();
    let decision_services = yaml_list(yaml_get(root, "decisionServices"))
        .iter()
        .filter_map(|value| {
            let map = yaml_map(value)?;
            let input_data = yaml_joined_list(yaml_get(map, "inputData"));
            let encapsulated_decisions = yaml_joined_list(yaml_get(map, "encapsulatedDecisions"));
            let output_decisions = yaml_joined_list(yaml_get(map, "outputDecisions"));
            let groups = [
                (!input_data.is_empty(), format!("输入: {input_data}")),
                (
                    !encapsulated_decisions.is_empty(),
                    format!("封装: {encapsulated_decisions}"),
                ),
                (
                    !output_decisions.is_empty(),
                    format!("输出: {output_decisions}"),
                ),
            ];
            Some(YamlDecisionService {
                name: yaml_text(yaml_get(map, "name")),
                element_type: "决策服务".into(),
                description: yaml_text(yaml_get(map, "description")),
                input_data: yaml_list(yaml_get(map, "inputData"))
                    .iter()
                    .map(|value| yaml_text(Some(value)))
                    .filter(|value| !value.is_empty())
                    .collect(),
                encapsulated_decisions: yaml_list(yaml_get(map, "encapsulatedDecisions"))
                    .iter()
                    .map(|value| yaml_text(Some(value)))
                    .filter(|value| !value.is_empty())
                    .collect(),
                output_decisions: yaml_list(yaml_get(map, "outputDecisions"))
                    .iter()
                    .map(|value| yaml_text(Some(value)))
                    .filter(|value| !value.is_empty())
                    .collect(),
                expression_summary: groups
                    .into_iter()
                    .filter_map(|(present, text)| present.then_some(text))
                    .collect::<Vec<_>>()
                    .join(" · "),
            })
        })
        .filter(|service| !service.name.is_empty())
        .collect::<Vec<_>>();
    let count = |key| yaml_list(yaml_get(root, key)).len();
    let input_names = yaml_list(yaml_get(root, "inputs"))
        .iter()
        .filter_map(|value| yaml_map(value).map(|map| yaml_text(yaml_get(map, "name"))))
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    let input_details = yaml_list(yaml_get(root, "inputs"))
        .iter()
        .filter_map(|value| {
            let map = yaml_map(value)?;
            Some(YamlField {
                name: yaml_text(yaml_get(map, "name")),
                element_type: "输入数据".into(),
                type_ref: yaml_text(yaml_get(map, "typeRef")),
                description: yaml_text(yaml_get(map, "description")),
            })
        })
        .filter(|field| !field.name.is_empty())
        .collect::<Vec<_>>();
    let type_details = yaml_list(yaml_get(root, "itemDefinitions"))
        .iter()
        .filter_map(|value| {
            let map = yaml_map(value)?;
            Some(YamlField {
                name: yaml_text(yaml_get(map, "name")),
                element_type: "数据类型".into(),
                type_ref: yaml_text(yaml_get(map, "type")),
                description: yaml_text(yaml_get(map, "description")),
            })
        })
        .filter(|field| !field.name.is_empty())
        .collect::<Vec<_>>();
    let bkm_names = yaml_list(yaml_get(root, "bkms"))
        .iter()
        .filter_map(|value| yaml_map(value).map(|map| yaml_text(yaml_get(map, "name"))))
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    let mut graph_nodes = Vec::new();
    for (index, name) in input_names.iter().enumerate() {
        graph_nodes.push(YamlGraphNode {
            name: name.clone(),
            kind: "input".into(),
            x: 34,
            y: 34 + index as i32 * 58,
        });
    }
    for (index, decision) in decisions.iter().enumerate() {
        graph_nodes.push(YamlGraphNode {
            name: decision.name.clone(),
            kind: "decision".into(),
            x: 322,
            y: 34 + index as i32 * 72,
        });
    }
    for (index, name) in bkm_names.iter().enumerate() {
        graph_nodes.push(YamlGraphNode {
            name: name.clone(),
            kind: "knowledge".into(),
            x: 610,
            y: 34 + index as i32 * 58,
        });
    }
    let position = |name: &str| graph_nodes.iter().find(|node| node.name == name).cloned();
    let mut graph_edges = Vec::new();
    for decision in &decisions {
        for source in decision
            .required_inputs
            .iter()
            .chain(decision.required_decisions.iter())
            .chain(decision.required_knowledge.iter())
        {
            if let (Some(source_node), Some(target_node)) =
                (position(source), position(&decision.name))
            {
                let (x1, y1, x2, y2) = yaml_graph_edge_anchors(&source_node, &target_node);
                graph_edges.push(YamlGraphEdge {
                    source: source.to_string(),
                    target: decision.name.clone(),
                    kind: source_node.kind,
                    x1,
                    y1,
                    x2,
                    y2,
                });
            }
        }
    }
    YamlSummary {
        name: yaml_text(yaml_get(root, "name")),
        namespace: yaml_text(yaml_get(root, "namespace")),
        description: yaml_text(yaml_get(root, "description")),
        item_definitions: count("itemDefinitions"),
        inputs: count("inputs"),
        input_details,
        type_details,
        decisions,
        graph_nodes,
        graph_edges,
        bkms,
        decision_services,
        knowledge_sources: count("knowledgeSources"),
        text_annotations: count("textAnnotations"),
        raw: Some(raw),
        error: None,
    }
}

#[component]
fn YamlArtifact(
    value: ReadSignal<Option<String>>,
    set_value: Option<WriteSignal<Option<String>>>,
    set_xml: Option<WriteSignal<Option<String>>>,
    set_source: Option<WriteSignal<String>>,
) -> impl IntoView {
    let view = RwSignal::new(YamlView::Model);
    let copy_status = RwSignal::new(CopyStatus::Idle);
    let source_text = RwSignal::new(value.get_untracked().unwrap_or_default());
    let source_error = RwSignal::new(None::<String>);
    let compile_state = RwSignal::new(None::<String>);
    let editing_source = RwSignal::new(false);
    Effect::new(move |_| {
        let next_value = value.get();
        source_text.set(next_value.clone().unwrap_or_default());
        if next_value.is_none() {
            source_error.set(None);
            compile_state.set(None);
            editing_source.set(false);
        }
    });
    let summary = Memo::new(move |_| {
        value
            .get()
            .map(|source| yaml_summary(&source))
            .unwrap_or_default()
    });
    let copy = move |_| {
        let Some(content) = value.get() else {
            return;
        };
        spawn_local(async move {
            let copied = if let Some(window) = web_sys::window() {
                JsFuture::from(window.navigator().clipboard().write_text(&content))
                    .await
                    .is_ok()
            } else {
                false
            };
            copy_status.set(if copied {
                CopyStatus::Copied
            } else {
                CopyStatus::Failed
            });
        });
    };
    view! {
        <div class="artifact-block yaml-artifact">
            <div class="artifact-heading">
                <span>"DSL YAML"</span>
                <div class="artifact-heading-actions">
                    <span class="artifact-type">"YAML"</span>
                    <button class="button button-quiet artifact-copy" on:click=copy title="复制 YAML">
                        {move || match copy_status.get() { CopyStatus::Idle => "复制", CopyStatus::Copied => "已复制", CopyStatus::Failed => "复制失败" }}
                    </button>
                </div>
            </div>
            <div class="yaml-view-tabs" role="tablist" aria-label="YAML 展示方式">
                <button class=move || if view.get() == YamlView::Model { "yaml-view-tab is-active" } else { "yaml-view-tab" } on:click=move |_| view.set(YamlView::Model)>"模型"</button>
                <button class=move || if view.get() == YamlView::Dependency { "yaml-view-tab is-active" } else { "yaml-view-tab" } on:click=move |_| view.set(YamlView::Dependency)>"决策依赖图"</button>
                <button class=move || if view.get() == YamlView::Structure { "yaml-view-tab is-active" } else { "yaml-view-tab" } on:click=move |_| view.set(YamlView::Structure)>"结构"</button>
                <button class=move || if view.get() == YamlView::Source { "yaml-view-tab is-active" } else { "yaml-view-tab" } on:click=move |_| view.set(YamlView::Source)>"源码"</button>
            </div>
            <Show when=move || compile_state.get().is_some()>
                <div class=move || if compile_state.get().as_deref() == Some("DMN XML 已同步") { "yaml-sync-status is-success" } else if compile_state.get().as_deref() == Some("YAML 已应用，XML 编译失败") { "yaml-sync-status is-error" } else { "yaml-sync-status" }>{move || compile_state.get().unwrap_or_default()}</div>
            </Show>
            <Show when=move || view.get() == YamlView::Model>
                <YamlModelView summary=summary />
            </Show>
            <Show when=move || view.get() == YamlView::Dependency>
                <YamlDependencyGraphView summary=summary />
            </Show>
            <Show when=move || view.get() == YamlView::Structure>
                <YamlStructureView summary=summary />
            </Show>
            <Show when=move || view.get() == YamlView::Source>
                <div class="yaml-view-content">
                    <Show
                        when=move || editing_source.get()
                        fallback=move || view! {
                            <div class="yaml-source-preview">
                                <pre class="code-view code-view-yaml yaml-source-view"><code inner_html=move || highlight_code(&value.get().unwrap_or_else(|| "等待生成...".into()), "yaml")></code></pre>
                                <div class="yaml-source-actions"><button class="button button-quiet" on:click=move |_| editing_source.set(true) disabled=move || set_value.is_none()>"编辑 YAML"</button></div>
                            </div>
                        }
                    >
                        <div class="yaml-source-editor">
                            <textarea class="yaml-source-input" prop:value=source_text on:input=move |event| { source_text.set(event_target_value(&event)); source_error.set(None); } placeholder="编辑 DSL YAML..."></textarea>
                            <div class="yaml-source-actions">
                                <Show when=move || source_error.get().is_some()><span class="yaml-source-error">{move || source_error.get().unwrap_or_default()}</span></Show>
                                <button class="button button-quiet" on:click=move |_| { source_text.set(value.get_untracked().unwrap_or_default()); source_error.set(None); editing_source.set(false); } >"取消"</button>
                                <button class="button button-primary" on:click=move |_| {
                                    let text = source_text.get();
                                    match serde_yaml::from_str::<YamlValue>(&text) {
                                        Ok(parsed) if yaml_map(&parsed).is_some() => {
                                            if let Some(set_value) = set_value { set_value.set(Some(text.clone())); }
                                            if let Some(set_source) = set_source { set_source.set(text.clone()); }
                                            compile_state.set(Some("正在编译 DMN XML...".into()));
                                            source_error.set(None);
                                            editing_source.set(false);
                                            if let (Some(set_xml), Some(token)) = (set_xml, auth_token().get_untracked()) {
                                                let dsl_text = source_text.get();
                                                spawn_local(async move {
                                                    match api::compile_dsl(&token, dsl_text).await {
                                                        Ok(xml) => { set_xml.set(Some(xml)); compile_state.set(Some("DMN XML 已同步".into())); }
                                                        Err(error) => { compile_state.set(Some("YAML 已应用，XML 编译失败".into())); source_error.set(Some(error)); }
                                                    }
                                                });
                                            } else {
                                                compile_state.set(Some("YAML 已应用".into()));
                                            }
                                            view.set(YamlView::Model);
                                        }
                                        Ok(_) => source_error.set(Some("YAML 根节点必须是对象".into())),
                                        Err(error) => source_error.set(Some(format!("YAML 解析失败：{error}"))),
                                    }
                                } disabled=move || set_value.is_none()>"应用 YAML"</button>
                            </div>
                        </div>
                    </Show>
                </div>
            </Show>
        </div>
    }
}

fn yaml_kind(value: &YamlValue) -> &'static str {
    match value {
        YamlValue::Mapping(_) => "对象",
        YamlValue::Sequence(_) => "数组",
        YamlValue::String(_) => "字符串",
        YamlValue::Number(_) => "数字",
        YamlValue::Bool(_) => "布尔值",
        YamlValue::Null => "空值",
        YamlValue::Tagged(_) => "值",
    }
}

fn yaml_preview(value: &YamlValue) -> String {
    match value {
        YamlValue::Mapping(map) => format!("{} 个字段", map.len()),
        YamlValue::Sequence(items) => format!("{} 项", items.len()),
        _ => yaml_text(Some(value)),
    }
}

fn indexed(values: &[String]) -> Vec<(usize, String)> {
    values.iter().cloned().enumerate().collect()
}

#[derive(Clone, PartialEq, Eq)]
struct YamlPathEntry {
    path: String,
    kind: String,
    preview: String,
    depth: usize,
}

fn collect_yaml_paths(
    value: &YamlValue,
    path: &str,
    depth: usize,
    output: &mut Vec<YamlPathEntry>,
) {
    if depth > 3 {
        return;
    }
    match value {
        YamlValue::Mapping(map) => {
            for (key, child) in map {
                let key = yaml_text(Some(key));
                let child_path = if path.is_empty() {
                    key
                } else {
                    format!("{path}.{key}")
                };
                output.push(YamlPathEntry {
                    path: child_path.clone(),
                    kind: yaml_kind(child).into(),
                    preview: yaml_preview(child),
                    depth: depth + 1,
                });
                collect_yaml_paths(child, &child_path, depth + 1, output);
            }
        }
        YamlValue::Sequence(items) => {
            for (index, child) in items.iter().enumerate() {
                let child_path = format!("{path}[{index}]");
                output.push(YamlPathEntry {
                    path: child_path.clone(),
                    kind: yaml_kind(child).into(),
                    preview: yaml_preview(child),
                    depth: depth + 1,
                });
                collect_yaml_paths(child, &child_path, depth + 1, output);
            }
        }
        _ => {}
    }
}

#[component]
fn YamlModelView(summary: Memo<YamlSummary>) -> impl IntoView {
    let selected = RwSignal::new(None::<String>);
    view! {
        <Show
            when=move || summary.get().error.is_some()
            fallback=move || view! {
                <div class="yaml-model-view yaml-view-content">
                    <div class="yaml-overview">
                        <div class="yaml-overview-title">
                            <span class="yaml-kicker">"DMN DEFINITIONS"</span>
                            <h3>{move || summary.get().name}</h3>
                            <p>{move || summary.get().description}</p>
                            <code>{move || summary.get().namespace}</code>
                        </div>
                        <div class="yaml-stat-grid">
                            <div><strong>{move || summary.get().inputs}</strong><span>"输入"</span></div>
                            <div><strong>{move || summary.get().decisions.len()}</strong><span>"决策"</span></div>
                            <div><strong>{move || summary.get().item_definitions}</strong><span>"数据类型"</span></div>
                            <div><strong>{move || summary.get().decision_services.len()}</strong><span>"决策服务"</span></div>
                        </div>
                    </div>
                    <YamlElementNavigator summary=summary selected=selected />
                    <YamlDataCatalog summary=summary />
                    <div class="yaml-section-heading"><span>"决策定义"</span><span>{move || format!("{} 个", summary.get().decisions.len())}</span></div>
                    <div class="yaml-decision-list">
                        <For
                            each=move || summary.get().decisions
                            key=|decision| decision.name.clone()
                            let:decision
                        >
                            <YamlDecisionView decision=decision selected=selected />
                        </For>
                    </div>
                    <div class="yaml-section-heading"><span>"业务知识模型"</span><span>{move || format!("{} 个", summary.get().bkms.len())}</span></div>
                    <div class="yaml-decision-list">
                        <For
                            each=move || summary.get().bkms
                            key=|bkm| bkm.name.clone()
                            let:bkm
                        >
                            <YamlKnowledgeModelView model=bkm selected=selected />
                        </For>
                        <Show when=move || summary.get().bkms.is_empty()>
                            <p class="yaml-empty-copy">"未定义业务知识模型"</p>
                        </Show>
                    </div>
                    <div class="yaml-section-heading"><span>"决策服务"</span><span>{move || format!("{} 个", summary.get().decision_services.len())}</span></div>
                    <div class="yaml-decision-list">
                        <For
                            each=move || summary.get().decision_services
                            key=|service| service.name.clone()
                            let:service
                        >
                            <YamlDecisionServiceView service=service selected=selected />
                        </For>
                        <Show when=move || summary.get().decision_services.is_empty()>
                            <p class="yaml-empty-copy">"未定义决策服务"</p>
                        </Show>
                    </div>
                </div>
            }
        >
            <div class="yaml-parse-error yaml-view-content">{move || summary.get().error.unwrap_or_else(|| "YAML 无法解析".into())}</div>
        </Show>
    }
}

#[component]
fn YamlDecisionView(decision: YamlDecision, selected: RwSignal<Option<String>>) -> impl IntoView {
    let has_table = !decision.rules.is_empty();
    let decision_name = decision.name.clone();
    let headers = RwSignal::new(decision.inputs.clone());
    let output_headers = RwSignal::new(decision.outputs.clone());
    let rows = RwSignal::new(
        decision
            .rules
            .clone()
            .into_iter()
            .enumerate()
            .collect::<Vec<_>>(),
    );
    view! {
        <article class=move || if selected.get().as_deref() == Some(decision_name.as_str()) { "yaml-decision-card is-selected" } else { "yaml-decision-card" }>
            <div class="yaml-decision-heading">
                <div><span class="yaml-node-badge">"D"</span><div><h4>{decision.name.clone()}</h4><p>{decision.description.clone()}</p></div></div>
                <div class="yaml-decision-meta"><span>{decision.element_type.clone()}</span><span>{decision.type_ref.clone()}</span><span>{if decision.hit_policy.is_empty() { "LITERAL".into() } else { decision.hit_policy.clone() }}</span></div>
            </div>
            <Show when=move || has_table fallback=move || view! {
                <div class="yaml-expression-card">
                    <div class="yaml-expression-type"><span>"表达式类型"</span><strong>{decision.expression_type.clone()}</strong></div>
                    <pre class="yaml-expression">{if decision.expression_summary.is_empty() { "该表达式未提供可直接预览的文本。".into() } else { decision.expression_summary.clone() }}</pre>
                </div>
            }>
                <div class="yaml-table-wrap">
                    <table class="yaml-decision-table">
                        <thead><tr><For each=move || headers.get() key=|header| header.clone() let:header><th>{header}</th></For><For each=move || output_headers.get() key=|header| format!("out-{header}") let:header><th class="yaml-output-header">{header}</th></For></tr></thead>
                        <tbody>
                            <For each=move || rows.get() key=|(index, _)| *index let:row>
                                <tr><For each=move || indexed(&row.1.inputs) key=|(index, _)| *index let:cell><td>{cell.1}</td></For><For each=move || indexed(&row.1.outputs) key=|(index, _)| format!("out-{index}") let:cell><td class="yaml-output-cell">{cell.1}</td></For></tr>
                            </For>
                        </tbody>
                    </table>
                </div>
            </Show>
        </article>
    }
}

#[component]
fn YamlKnowledgeModelView(
    model: YamlKnowledgeModel,
    selected: RwSignal<Option<String>>,
) -> impl IntoView {
    let model_name = model.name.clone();
    let parameters = model.parameters.clone();
    let has_parameters = !parameters.is_empty();
    let parameter_text = parameters.join(", ");
    view! {
        <article class=move || if selected.get().as_deref() == Some(model_name.as_str()) { "yaml-decision-card is-selected" } else { "yaml-decision-card" }>
            <div class="yaml-decision-heading">
                <div><span class="yaml-node-badge yaml-node-badge-knowledge">"K"</span><div><h4>{model.name.clone()}</h4><p>{model.description.clone()}</p></div></div>
                <div class="yaml-decision-meta"><span>{model.element_type.clone()}</span><span>{model.type_ref.clone()}</span><span>{model.expression_type.clone()}</span></div>
            </div>
            <Show when=move || has_parameters>
                <div class="yaml-definition-line"><strong>"参数"</strong><span>{parameter_text.clone()}</span></div>
            </Show>
            <div class="yaml-expression-card">
                <div class="yaml-expression-type"><span>"表达式预览"</span><strong>{model.expression_type.clone()}</strong></div>
                <pre class="yaml-expression">{if model.expression_summary.is_empty() { "该表达式未提供可直接预览的文本。".into() } else { model.expression_summary.clone() }}</pre>
            </div>
        </article>
    }
}

#[component]
fn YamlDecisionServiceView(
    service: YamlDecisionService,
    selected: RwSignal<Option<String>>,
) -> impl IntoView {
    let service_name = service.name.clone();
    view! {
        <article class=move || if selected.get().as_deref() == Some(service_name.as_str()) { "yaml-decision-card is-selected" } else { "yaml-decision-card" }>
            <div class="yaml-decision-heading">
                <div><span class="yaml-node-badge yaml-node-badge-service">"S"</span><div><h4>{service.name.clone()}</h4><p>{service.description.clone()}</p></div></div>
                <div class="yaml-decision-meta"><span>{service.element_type.clone()}</span></div>
            </div>
            <div class="yaml-expression-card">
                <div class="yaml-expression-type"><span>"服务范围"</span><strong>"decisionService"</strong></div>
                <pre class="yaml-expression">{if service.expression_summary.is_empty() { "未配置输入、封装决策或输出决策。".into() } else { service.expression_summary.clone() }}</pre>
            </div>
        </article>
    }
}

#[component]
fn YamlElementNavigator(
    summary: Memo<YamlSummary>,
    selected: RwSignal<Option<String>>,
) -> impl IntoView {
    view! {
        <div class="yaml-navigator">
            <div class="yaml-section-heading"><span>"元素导航"</span><span>"全部定义元素"</span></div>
            <div class="yaml-navigator-list">
                <For each=move || summary.get().graph_nodes key=|node| node.name.clone() children=move |node| {
                    let node_name = node.name.clone();
                    let node_kind = node.kind.clone();
                    let class_name = node_name.clone();
                    let click_name = node_name.clone();
                    view! {
                        <button class=move || format!("yaml-nav-item yaml-nav-{} {}", node_kind, if selected.get().as_deref() == Some(class_name.as_str()) { "is-selected" } else { "" }) on:click=move |_| selected.set(Some(click_name.clone()))>
                            <span class="yaml-nav-dot"></span><span>{node_name.clone()}</span>
                        </button>
                    }
                } />
                <For each=move || {
                    summary.get().bkms.iter().map(|bkm| YamlGraphNode { name: bkm.name.clone(), kind: "knowledge".into(), ..Default::default() }).collect::<Vec<_>>()
                } key=|node| node.name.clone() children=move |node| {
                    let node_name = node.name.clone();
                    let class_name = node_name.clone();
                    let click_name = node_name.clone();
                    view! {
                        <button class=move || format!("yaml-nav-item yaml-nav-knowledge {}", if selected.get().as_deref() == Some(class_name.as_str()) { "is-selected" } else { "" }) on:click=move |_| selected.set(Some(click_name.clone()))>
                            <span class="yaml-nav-dot"></span><span>{node_name.clone()}</span>
                        </button>
                    }
                } />
                <For each=move || {
                    summary.get().decision_services.iter().map(|service| YamlGraphNode { name: service.name.clone(), kind: "service".into(), ..Default::default() }).collect::<Vec<_>>()
                } key=|node| node.name.clone() children=move |node| {
                    let node_name = node.name.clone();
                    let class_name = node_name.clone();
                    let click_name = node_name.clone();
                    view! {
                        <button class=move || format!("yaml-nav-item yaml-nav-service {}", if selected.get().as_deref() == Some(class_name.as_str()) { "is-selected" } else { "" }) on:click=move |_| selected.set(Some(click_name.clone()))>
                            <span class="yaml-nav-dot"></span><span>{node_name.clone()}</span>
                        </button>
                    }
                } />
            </div>
        </div>
    }
}

#[component]
fn YamlDependencyGraphView(summary: Memo<YamlSummary>) -> impl IntoView {
    let selected = RwSignal::new(None::<String>);
    view! {
        <div class="yaml-view-content">
            <Show
                when=move || summary.get().error.is_some()
                fallback=move || view! {
                    <YamlDependencyGraph summary=summary selected=selected />
                }
            >
                <div class="yaml-parse-error">{move || summary.get().error.unwrap_or_else(|| "YAML 无法解析".into())}</div>
            </Show>
        </div>
    }
}

#[component]
fn YamlDependencyGraph(
    summary: Memo<YamlSummary>,
    selected: RwSignal<Option<String>>,
) -> impl IntoView {
    view! {
        <div class="yaml-graph-panel">
            <div class="yaml-section-heading"><span>"决策依赖图"</span><span>{move || format!("{} 条依赖", summary.get().graph_edges.len())}</span></div>
            <svg
                class="yaml-dependency-graph"
                viewBox=move || format!("0 0 {YAML_GRAPH_WIDTH} {}", yaml_graph_height(&summary.get().graph_nodes))
                preserveAspectRatio="xMinYMin meet"
                role="img"
                aria-label="DMN 决策依赖图"
            >
                    <defs>
                        <marker id="yaml-graph-arrow" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="6" markerHeight="6" orient="auto">
                            <path d="M 0 0 L 10 5 L 0 10 z" fill="context-stroke" />
                        </marker>
                    </defs>
                    <For each=move || summary.get().graph_edges key=|edge| format!("{}->{}", edge.source, edge.target) let:edge>
                        <path
                            class=format!("yaml-graph-edge yaml-graph-edge-{}", edge.kind)
                            d=yaml_graph_edge_path(&edge)
                            marker-end="url(#yaml-graph-arrow)"
                        />
                    </For>
                    <For each=move || summary.get().graph_nodes key=|node| node.name.clone() children=move |node| {
                        let node_name = node.name.clone();
                        let node_kind = node.kind.clone();
                        let class_name = node_name.clone();
                        let click_name = node_name.clone();
                        let label = yaml_graph_label(&node_name);
                        let knowledge_points = RwSignal::new(yaml_graph_knowledge_points(&node));
                        let is_input = node_kind == "input";
                        let is_decision = node_kind == "decision";
                        let is_knowledge = node_kind == "knowledge";
                        view! {
                            <g class=move || format!("yaml-graph-node yaml-graph-node-{} {}", node_kind, if selected.get().as_deref() == Some(class_name.as_str()) { "is-selected" } else { "" }) on:click=move |_| selected.set(Some(click_name.clone()))>
                                <title>{node_name.clone()}</title>
                                <Show when=move || is_input>
                                    <rect x=node.x.to_string() y=node.y.to_string() width=YAML_GRAPH_NODE_WIDTH.to_string() height=YAML_GRAPH_NODE_HEIGHT.to_string() rx="25" ry="25" />
                                </Show>
                                <Show when=move || is_decision>
                                    <rect x=node.x.to_string() y=node.y.to_string() width=YAML_GRAPH_NODE_WIDTH.to_string() height=YAML_GRAPH_NODE_HEIGHT.to_string() rx="2" />
                                </Show>
                                <Show when=move || is_knowledge>
                                    <polygon points=move || knowledge_points.get() />
                                </Show>
                                <text x={(node.x + YAML_GRAPH_NODE_WIDTH / 2).to_string()} y={(node.y + 30).to_string()}>{label}</text>
                            </g>
                        }
                    } />
            </svg>
        </div>
    }
}

#[component]
fn YamlDataCatalog(summary: Memo<YamlSummary>) -> impl IntoView {
    view! {
        <div class=move || if summary.get().type_details.is_empty() { "yaml-catalog-grid is-single" } else { "yaml-catalog-grid" }>
            <section class="yaml-catalog-section">
                <div class="yaml-section-heading"><span>"输入数据"</span><span>{move || format!("{} 个", summary.get().input_details.len())}</span></div>
                <div class="yaml-field-list">
                    <For each=move || summary.get().input_details key=|field| field.name.clone() let:field>
                        <div class="yaml-field-row"><span class="yaml-field-dot yaml-field-input"></span><div><strong>{field.name}</strong><small>{field.element_type}{if field.description.is_empty() { String::new() } else { format!(" · {}", field.description) }}</small></div><code>{field.type_ref}</code></div>
                    </For>
                    <Show when=move || summary.get().input_details.is_empty()><p class="yaml-empty-copy">"未定义输入数据"</p></Show>
                </div>
            </section>
            <Show when=move || !summary.get().type_details.is_empty() fallback=move || view! {
                <div class="yaml-catalog-empty"><span class="yaml-field-dot yaml-field-type"></span><span>"数据类型"</span><small>"未定义自定义数据类型"</small></div>
            }>
                <section class="yaml-catalog-section">
                    <div class="yaml-section-heading"><span>"数据类型"</span><span>{move || format!("{} 个", summary.get().type_details.len())}</span></div>
                    <div class="yaml-field-list">
                        <For each=move || summary.get().type_details key=|field| field.name.clone() let:field>
                            <div class="yaml-field-row"><span class="yaml-field-dot yaml-field-type"></span><div><strong>{field.name}</strong><small>{field.element_type}{if field.description.is_empty() { String::new() } else { format!(" · {}", field.description) }}</small></div><code>{field.type_ref}</code></div>
                        </For>
                    </div>
                </section>
            </Show>
        </div>
    }
}

#[component]
fn YamlStructureView(summary: Memo<YamlSummary>) -> impl IntoView {
    let query = RwSignal::new(String::new());
    let entries = move || {
        let query_value = query.get().to_lowercase();
        let mut all = Vec::new();
        if let Some(raw) = summary.get().raw.as_ref() {
            collect_yaml_paths(raw, "", 0, &mut all);
        }
        if query_value.trim().is_empty() {
            all
        } else {
            all.into_iter()
                .filter(|entry| {
                    entry.path.to_lowercase().contains(&query_value)
                        || entry.preview.to_lowercase().contains(&query_value)
                })
                .collect()
        }
    };
    let copy_path = move |path: String| {
        spawn_local(async move {
            if let Some(window) = web_sys::window() {
                let _ = JsFuture::from(window.navigator().clipboard().write_text(&path)).await;
            }
        });
    };
    view! {
        <div class="yaml-structure-view yaml-view-content">
            <div class="yaml-structure-toolbar">
                <span>"结构树"</span>
                <input class="yaml-tree-search" type="search" placeholder="搜索字段或值" prop:value=query on:input=move |event| query.set(event_target_value(&event)) />
            </div>
            <div class="yaml-tree-list">
                <For each=entries key=|entry| entry.path.clone() children=move |entry| {
                    let path = entry.path.clone();
                    let copy_value = entry.path.clone();
                    view! {
                        <div class="yaml-tree-row" style=format!("padding-left:{}px", 10 + entry.depth * 14)>
                            <span class="yaml-tree-caret">"•"</span>
                            <code>{path}</code>
                            <span class="yaml-tree-kind">{entry.kind}</span>
                            <span class="yaml-tree-preview">{entry.preview}</span>
                            <button class="yaml-tree-copy" title="复制路径" on:click=move |_| copy_path(copy_value.clone())>"复制路径"</button>
                        </div>
                    }
                } />
            </div>
            <Show when=move || entries().is_empty()>
                <p class="yaml-empty-copy">"没有匹配的字段"</p>
            </Show>
        </div>
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn token_span(class: &str, value: &str) -> String {
    format!(
        "<span class=\"syntax-{class}\">{}</span>",
        escape_html(value)
    )
}

fn highlight_value(value: &str, comment_marker: Option<char>) -> String {
    let mut output = String::new();
    let mut rest = value;
    while !rest.is_empty() {
        let quote = rest.find(|character| character == '\'' || character == '"');
        let comment = comment_marker.and_then(|marker| rest.find(marker));
        let next = match (quote, comment) {
            (Some(q), Some(c)) => Some((q.min(c), q <= c)),
            (Some(q), None) => Some((q, true)),
            (None, Some(c)) => Some((c, false)),
            (None, None) => None,
        };
        let Some((index, is_quote)) = next else {
            output.push_str(&escape_html(rest));
            break;
        };
        output.push_str(&escape_html(&rest[..index]));
        if is_quote {
            let quote = rest.as_bytes()[index] as char;
            let end = rest[index + 1..]
                .find(quote)
                .map(|offset| index + 1 + offset + 1)
                .unwrap_or(rest.len());
            output.push_str(&token_span("string", &rest[index..end]));
            rest = &rest[end..];
        } else {
            output.push_str(&token_span("comment", rest[index..].trim_end_matches('\n')));
            break;
        }
    }
    output
}

fn highlight_yaml_line(line: &str) -> String {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];
    let key_end = trimmed.find(':');
    if let Some(key_end) = key_end {
        let key = &trimmed[..key_end];
        if !key.is_empty() && !key.chars().any(char::is_whitespace) {
            return format!(
                "{}{}:<span class=\"syntax-value\">{}</span>",
                escape_html(indent),
                token_span("key", key),
                highlight_value(&trimmed[key_end + 1..], Some('#'))
            );
        }
    }
    highlight_value(line, Some('#'))
}

fn highlight_xml_text(value: &str) -> String {
    let mut output = String::new();
    let mut rest = value;
    while let Some(start) = rest.find('&') {
        output.push_str(&escape_html(&rest[..start]));
        let entity = &rest[start..];
        let Some(end) = entity.find(';') else {
            output.push_str(&escape_html(entity));
            return output;
        };
        let candidate = &entity[..=end];
        if candidate[1..candidate.len() - 1]
            .chars()
            .all(|character| character.is_alphanumeric() || character == '#' || character == 'x')
        {
            output.push_str(&token_span("entity", candidate));
        } else {
            output.push_str(&escape_html(candidate));
        };
        rest = &entity[end + 1..];
    }
    output.push_str(&escape_html(rest));
    output
}

fn xml_token_end(value: &str) -> usize {
    value
        .find(|character: char| {
            character.is_whitespace() || matches!(character, '=' | '/' | '?' | '>')
        })
        .unwrap_or(value.len())
}

fn highlight_xml_tag(tag: &str) -> String {
    let mut output = String::new();
    let mut rest = &tag[1..tag.len() - 1];
    output.push_str(&token_span("punctuation", "<"));

    if let Some(stripped) = rest.strip_prefix('/') {
        output.push_str(&token_span("punctuation", "/"));
        rest = stripped;
    } else if let Some(stripped) = rest.strip_prefix('?') {
        output.push_str(&token_span("punctuation", "?"));
        rest = stripped;
    }

    let name_end = xml_token_end(rest);
    if name_end == 0 {
        return format!("{}{}", output, escape_html(rest));
    }
    output.push_str(&token_span("tag-name", &rest[..name_end]));
    rest = &rest[name_end..];

    while !rest.is_empty() {
        let whitespace_len = rest.len() - rest.trim_start().len();
        output.push_str(&escape_html(&rest[..whitespace_len]));
        rest = &rest[whitespace_len..];
        if rest.is_empty() {
            break;
        }
        if let Some(stripped) = rest.strip_prefix('/') {
            output.push_str(&token_span("punctuation", "/"));
            rest = stripped;
            continue;
        }
        if let Some(stripped) = rest.strip_prefix('?') {
            output.push_str(&token_span("punctuation", "?"));
            rest = stripped;
            continue;
        }

        let attribute_end = xml_token_end(rest);
        if attribute_end == 0 {
            output.push_str(&escape_html(&rest[..1]));
            rest = &rest[1..];
            continue;
        }
        output.push_str(&token_span("attribute", &rest[..attribute_end]));
        rest = &rest[attribute_end..];

        let before_equals = rest.len() - rest.trim_start().len();
        output.push_str(&escape_html(&rest[..before_equals]));
        rest = &rest[before_equals..];
        let Some(stripped) = rest.strip_prefix('=') else {
            continue;
        };
        output.push_str(&token_span("punctuation", "="));
        rest = stripped;

        let before_value = rest.len() - rest.trim_start().len();
        output.push_str(&escape_html(&rest[..before_value]));
        rest = &rest[before_value..];
        if let Some(quote) = rest
            .chars()
            .next()
            .filter(|quote| *quote == '\'' || *quote == '"')
        {
            let value_end = rest[quote.len_utf8()..]
                .find(quote)
                .map(|offset| quote.len_utf8() + offset + quote.len_utf8())
                .unwrap_or(rest.len());
            output.push_str(&token_span("attribute-value", &rest[..value_end]));
            rest = &rest[value_end..];
        } else {
            let value_end = xml_token_end(rest);
            output.push_str(&token_span("attribute-value", &rest[..value_end]));
            rest = &rest[value_end..];
        }
    }

    output.push_str(&token_span("punctuation", ">"));
    output
}

fn highlight_xml(source: &str) -> String {
    let mut output = String::new();
    let mut rest = source;
    while let Some(start) = rest.find('<') {
        output.push_str(&highlight_xml_text(&rest[..start]));
        rest = &rest[start..];

        let (prefix, terminator, class) = if rest.starts_with("<!--") {
            ("<!--", "-->", "comment")
        } else if rest.starts_with("<![CDATA[") {
            ("<![CDATA[", "]]>", "cdata")
        } else if rest.starts_with("<!") {
            ("<!", ">", "meta")
        } else {
            let Some(end) = rest.find('>') else {
                output.push_str(&escape_html(rest));
                return output;
            };
            output.push_str(&highlight_xml_tag(&rest[..=end]));
            rest = &rest[end + 1..];
            continue;
        };

        let content_start = prefix.len();
        let end = rest[content_start..]
            .find(terminator)
            .map(|offset| content_start + offset + terminator.len())
            .unwrap_or(rest.len());
        output.push_str(&token_span(class, &rest[..end]));
        rest = &rest[end..];
    }
    output.push_str(&highlight_xml_text(rest));
    output
}

fn highlight_json(source: &str) -> String {
    let mut output = String::new();
    let bytes = source.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        let character = source[index..]
            .chars()
            .next()
            .expect("JSON highlighter index must remain on a character boundary");
        if character == '"' {
            let start = index;
            index += 1;
            let mut escaped = false;
            while index < bytes.len() {
                let current = bytes[index] as char;
                index += 1;
                if escaped {
                    escaped = false;
                } else if current == '\\' {
                    escaped = true;
                } else if current == '"' {
                    break;
                }
            }
            let end = index;
            let mut lookahead = end;
            while lookahead < bytes.len() && (bytes[lookahead] as char).is_ascii_whitespace() {
                lookahead += 1;
            }
            let class = if lookahead < bytes.len() && bytes[lookahead] as char == ':' {
                "key"
            } else {
                "string"
            };
            output.push_str(&token_span(class, &source[start..end]));
            continue;
        }

        if matches!(character, '{' | '}' | '[' | ']' | ',' | ':') {
            output.push_str(&token_span("punctuation", &source[index..index + 1]));
            index += 1;
            continue;
        }

        if character == '-' || character.is_ascii_digit() {
            let start = index;
            index += 1;
            while index < bytes.len()
                && matches!(
                    bytes[index] as char,
                    '0'..='9' | '.' | 'e' | 'E' | '+' | '-'
                )
            {
                index += 1;
            }
            output.push_str(&token_span("number", &source[start..index]));
            continue;
        }

        let remaining = &source[index..];
        let keyword = ["true", "false", "null"].into_iter().find(|keyword| {
            remaining.starts_with(keyword) && {
                remaining
                    .as_bytes()
                    .get(keyword.len())
                    .map_or(true, |next| {
                        !(next.is_ascii_alphanumeric() || *next == b'_')
                    })
            }
        });
        if let Some(keyword) = keyword {
            let class = if keyword == "null" { "null" } else { "boolean" };
            output.push_str(&token_span(class, keyword));
            index += keyword.len();
            continue;
        }

        let start = index;
        index += character.len_utf8();
        while index < bytes.len() {
            let current = source[index..]
                .chars()
                .next()
                .expect("JSON highlighter index must remain on a character boundary");
            if current == '"'
                || matches!(current, '{' | '}' | '[' | ']' | ',' | ':')
                || current == '-'
                || current.is_ascii_digit()
            {
                break;
            }
            if ["true", "false", "null"]
                .iter()
                .any(|keyword| source[index..].starts_with(keyword))
            {
                break;
            }
            index += current.len_utf8();
        }
        output.push_str(&escape_html(&source[start..index]));
    }

    output
}

fn highlight_code(source: &str, language: &str) -> String {
    if language == "json" {
        highlight_json(source)
    } else if language == "xml" {
        highlight_xml(source)
    } else {
        source
            .lines()
            .map(highlight_yaml_line)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_highlighting_separates_xml_tokens_and_escapes_content() {
        let highlighted = highlight_xml(
            "<?xml version=\"1.0\"?>\n<!-- decision\nmodel -->\n<dmn:decision name=\"Loan &amp; Approval\"><![CDATA[1 < 2]]></dmn:decision>",
        );

        assert!(highlighted.contains("syntax-tag-name\">xml"));
        assert!(highlighted.contains("syntax-attribute\">version"));
        assert!(highlighted.contains("syntax-attribute-value\">&quot;1.0&quot;"));
        assert!(highlighted.contains("syntax-comment"));
        assert!(highlighted.contains("syntax-cdata"));
        assert!(highlighted.contains("syntax-tag-name\">dmn:decision"));
        assert!(highlighted.contains("Loan &amp;amp; Approval"));
        assert!(!highlighted.contains("<dmn:decision"));
    }

    #[test]
    fn json_highlighting_separates_tokens_and_escapes_plain_text() {
        let highlighted = highlight_code(
            r#"{
  "Age": 35,
  "approved": true,
  "reason": null,
  "note": "A < B"
}"#,
            "json",
        );

        assert!(highlighted.contains("syntax-key\">&quot;Age&quot;"));
        assert!(highlighted.contains("syntax-number\">35"));
        assert!(highlighted.contains("syntax-boolean\">true"));
        assert!(highlighted.contains("syntax-null\">null"));
        assert!(highlighted.contains("A &lt; B"));
        assert!(!highlighted.contains("<script"));
    }

    #[test]
    fn yaml_summary_includes_all_definition_types_and_invocation_preview() {
        let summary = yaml_summary(
            r#"
name: Pricing
inputs:
  - name: Age
    typeRef: number
decisions:
  - name: Premium
    typeRef: number
    logic:
      type: invocation
      bkm: CalculatePremium
      bindings:
        - parameter: age
          expression: Age
bkms:
  - name: CalculatePremium
    parameters:
      - name: age
        typeRef: number
    logic:
      type: functionDefinition
      parameters:
        - name: age
          typeRef: number
      body:
        type: literalExpression
        expression: age * 2
decisionServices:
  - name: PricingService
    inputData: [Age]
    encapsulatedDecisions: [Premium]
    outputDecisions: [Premium]
"#,
        );

        assert_eq!(summary.decisions[0].element_type, "决策");
        assert_eq!(summary.decisions[0].expression_type, "invocation");
        assert_eq!(
            summary.decisions[0].expression_summary,
            "CalculatePremium(age = Age)"
        );
        assert_eq!(summary.bkms.len(), 1);
        assert_eq!(summary.bkms[0].element_type, "业务知识模型");
        assert!(summary.bkms[0].expression_summary.contains("age * 2"));
        assert_eq!(summary.decision_services.len(), 1);
        assert_eq!(summary.decision_services[0].element_type, "决策服务");
        assert!(
            summary.decision_services[0]
                .expression_summary
                .contains("封装: Premium")
        );
        assert_eq!(summary.input_details[0].element_type, "输入数据");
    }
}
