mod presentation;

use leptos::prelude::*;
use std::collections::BTreeMap;

use crate::components::common::{SelectOption, WorkbenchSelect};
use crate::models::{CreateDecisionTestCase, DecisionTestCase};
use crate::workflow::{
    Clarification, DecisionBrief, DecisionField, DecisionPlan, RequirementItem,
    SemanticTestProposal, TesterJudgeReview, TesterReport,
};
use presentation::{business_risk, business_type, field_label};

#[component]
pub fn ClarificationInput(
    item: Clarification,
    answers: ReadSignal<BTreeMap<String, String>>,
    set_answers: WriteSignal<BTreeMap<String, String>>,
    #[prop(into)] disabled: Signal<bool>,
) -> impl IntoView {
    let id_for_value = item.id.clone();
    let id_for_change = item.id.clone();
    let control_id = format!("clarification-{}", item.id);
    let input_id = control_id.clone();
    let control = if item.options.is_empty() {
        view! {
            <input
                id=input_id
                type="text"
                disabled=move || disabled.get()
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
            <WorkbenchSelect
                id=input_id
                class="clarification-select"
                label=item.question.clone()
                options=Signal::derive(move || {
                    let mut select_options = vec![SelectOption::new("", "请选择")];
                    select_options.extend(options.get().into_iter().map(|option| SelectOption::new(option.clone(), option)));
                    select_options
                })
                value=Signal::derive(move || answers.get().get(&id_for_value).cloned().unwrap_or_default())
                disabled=disabled
                placeholder=Signal::derive(|| "请选择".to_string())
                on_change=Callback::new(move |value| {
                    set_answers.update(|answers| { answers.insert(id_for_change.clone(), value); });
                })
            />
        }
        .into_any()
    };
    view! {
        <div class=if item.required { "clarification-item is-required" } else { "clarification-item" }>
            <label class="analysis-question-title" for=control_id>{item.question}<b class=if item.required { "analysis-question-required" } else { "analysis-question-optional" }>{if item.required { "需要回答" } else { "可选补充" }}</b></label>
            {(!item.reason.trim().is_empty()).then(|| view! { <small>{format!("为什么需要确认：{}", item.reason)}</small> })}
            {control}
        </div>
    }
}

#[component]
pub fn WorkflowFieldRow(
    field: DecisionField,
    #[prop(default = false)] output: bool,
) -> impl IntoView {
    let display_name = field_label(&field.label, &field.name);
    let value_type = business_type(&field.type_ref);
    let unit = if field.unit.is_empty() {
        None
    } else {
        Some(field.unit)
    };

    view! {
        <div class="analysis-field-row">
            <div class="analysis-field-copy">
                <div class="analysis-field-title"><strong>{display_name}</strong><span>{value_type}</span></div>
                {(!field.description.trim().is_empty()).then(|| view! { <p>{field.description}</p> })}
            </div>
            <div class="analysis-field-meta">
                {(!output).then(|| view! { <span class=if field.required { "is-required" } else { "" }>{if field.required { "必须提供" } else { "按需提供" }}</span> })}
                {unit.map(|value| view! { <span>{format!("单位：{value}")}</span> })}
            </div>
        </div>
    }
}

#[component]
pub fn DecisionBriefView(
    brief: DecisionBrief,
    #[prop(default = 0)] question_count: usize,
) -> impl IntoView {
    let (risk_label, risk_tone) = business_risk(&brief.risk_level);
    let inputs = RwSignal::new(brief.inputs);
    let outputs = RwSignal::new(brief.outputs);
    let constraints = RwSignal::new(brief.constraints);
    let assumptions = RwSignal::new(brief.assumptions);
    let terms = RwSignal::new(brief.terms);
    let detected_patterns = RwSignal::new(brief.detected_patterns);
    let requirement_items = RwSignal::new(brief.requirement_items);
    let input_count = inputs.get_untracked().len();
    let output_count = outputs.get_untracked().len();

    view! {
        <div class="analysis-result">
        <section class="analysis-business-summary" aria-label="需求分析摘要">
            <div class="analysis-summary-copy">
                <span class="analysis-section-label">"这次要解决什么问题"</span>
                <h2>{brief.goal}</h2>
                {(!brief.description.trim().is_empty()).then(|| view! { <p>{brief.description}</p> })}
                <div class="analysis-summary-facts">
                    <span>{format!("需要 {input_count} 项业务信息")}</span>
                    <span>{format!("得到 {output_count} 项判断结果")}</span>
                    <span>{format!("{} 条关键规则", constraints.get_untracked().len())}</span>
                </div>
            </div>
            <aside class=format!("analysis-risk analysis-risk-{risk_tone}")>
                <span>"业务风险提示"</span><strong>{risk_label}</strong>
                <small>"基于当前需求的初步判断，不代表最终审批或合规结论。"</small>
            </aside>
        </section>
        <div class="analysis-priority-grid">
            <section class="analysis-card analysis-rules">
                <div class="analysis-card-heading"><h3>"关键业务规则"</h3><span>"优先核对"</span></div>
                <p class="analysis-section-description">"重点确认判断条件、数值边界和例外情况是否符合实际业务。"</p>
                <Show when=move || !constraints.get().is_empty() fallback=|| view! { <p class="analysis-empty">"尚未提取到明确规则，请补充判断条件，避免直接生成不完整的方案。"</p> }>
                    <ol class="analysis-rule-list"><For each=move || constraints.get() key=|item| item.clone() let:item><li>{item}</li></For></ol>
                </Show>
            </section>
            <section class="analysis-card analysis-attention">
                <div class="analysis-card-heading"><h3>"需要特别留意"</h3></div>
                <Show when=move || !assumptions.get().is_empty() fallback=|| view! { <p class="analysis-empty">"当前未列出额外假设，请仍核对是否遗漏业务场景或例外情况。"</p> }>
                    <p class="analysis-section-description">"以下是分析时采用的假设，并非已经确认的业务规则。"</p>
                    <ul class="analysis-assumption-list"><For each=move || assumptions.get() key=|item| item.clone() let:item><li>{item}</li></For></ul>
                </Show>
                {(question_count > 0).then(|| view! { <p class="analysis-question-hint">{format!("还有 {question_count} 项业务问题，请在下方逐项核对并补充回答。")}</p> })}
            </section>
        </div>
        <Show when=move || !requirement_items.get().is_empty()>
            <details class="analysis-details requirement-reference-list">
                <summary>"需求条目"<span>{move || format!("{} 项", requirement_items.get().len())}</span></summary>
                <ul><For each=move || requirement_items.get() key=|item| item.id.clone() let:item>
                    <li><code>{item.id}</code><span>{item.text}</span><small>{item.source}</small></li>
                </For></ul>
            </details>
        </Show>
        <Show when=move || !detected_patterns.get().is_empty()>
            <section class="analysis-card analysis-patterns" aria-label="识别到的业务模式">
                <div class="analysis-card-heading"><h3>"识别到的业务模式"</h3><span>"启发式线索"</span></div>
                <p class="analysis-section-description">"这些模式用于辅助拆解需求，最终规则仍以你确认的业务事实为准。"</p>
                <div class="analysis-pattern-list">
                    <For each=move || detected_patterns.get() key=|pattern| pattern.pattern_type.clone() let:pattern>
                        {let label = match pattern.pattern_type.as_str() {
                            "tree" => "决策树",
                            "matrix" => "评分矩阵",
                            "workflow" => "流程序列",
                            "threshold" => "阈值规则",
                            _ => "业务模式",
                        };
                        view! {
                            <article class="analysis-pattern-item">
                                <div><strong>{label}</strong><span>{format!("置信度 {}%", pattern.confidence_percent)}</span></div>
                                <small>{if pattern.matched_keywords.is_empty() { "基于结构特征识别".to_string() } else { format!("命中关键词：{}", pattern.matched_keywords.join("、")) }}</small>
                            </article>
                        }}
                    </For>
                </div>
            </section>
        </Show>
        <div class="analysis-information-grid">
            <section class="analysis-card">
                <div class="analysis-card-heading"><h3>"需要提供的信息"</h3><span>{format!("{input_count} 项")}</span></div>
                <p class="analysis-section-description">"业务办理时，需要准备哪些资料或事实。"</p>
                <Show
                    when=move || !inputs.get().is_empty()
                    fallback=|| view! { <p class="analysis-empty">"暂未识别到所需信息，请核对业务资料是否完整。"</p> }
                >
                    <For each=move || inputs.get() key=|field| field.name.clone() let:field>
                        <WorkflowFieldRow field=field />
                    </For>
                </Show>
            </section>
            <section class="analysis-card analysis-outcomes">
                <div class="analysis-card-heading"><h3>"最终会得到什么"</h3><span>{format!("{output_count} 项")}</span></div>
                <p class="analysis-section-description">"完成判断后，业务人员可以获得哪些结论。"</p>
                <Show
                    when=move || !outputs.get().is_empty()
                    fallback=|| view! { <p class="analysis-empty">"暂未明确判断结果，请补充希望得到的业务结论。"</p> }
                >
                    <For each=move || outputs.get() key=|field| field.name.clone() let:field>
                        <WorkflowFieldRow field=field output=true />
                    </For>
                </Show>
            </section>
        </div>
        <Show when=move || !terms.get().is_empty()>
            <details class="analysis-details">
                <summary>"业务用语说明"<span>{move || format!("{} 项", terms.get().len())}</span></summary>
                <dl class="analysis-terms"><For each=move || terms.get() key=|item| item.term.clone() let:item><div><dt>{item.term}</dt><dd>{item.meaning}</dd></div></For></dl>
            </details>
        </Show>
        <details class="analysis-details analysis-technical">
            <summary>"查看技术字段对照"<span>"供实施人员使用"</span></summary>
            <p class="analysis-section-description">"保留原始字段标识与类型，业务核对时无需填写或修改。"</p>
            <div class="analysis-technical-grid">
                <section><h4>"所需信息"</h4><For each=move || inputs.get() key=|field| field.name.clone() let:field><div><strong>{field_label(&field.label, &field.name)}</strong><code>{field.name}</code><code>{field.type_ref}</code></div></For></section>
                <section><h4>"判断结果"</h4><For each=move || outputs.get() key=|field| field.name.clone() let:field><div><strong>{field_label(&field.label, &field.name)}</strong><code>{field.name}</code><code>{field.type_ref}</code></div></For></section>
            </div>
        </details>
        </div>
    }
}

#[component]
pub fn RequirementAnalysisProgress(
    prompt: ReadSignal<String>,
    agent_activity: ReadSignal<String>,
    orchestration_stage: ReadSignal<usize>,
    orchestration_events: ReadSignal<Vec<String>>,
    planning: ReadSignal<bool>,
    on_cancel: Callback<()>,
) -> impl IntoView {
    view! {
        <section class="requirement-analysis-progress" aria-live="polite">
            <div class="analysis-progress-heading">
                <span class="analysis-progress-indicator" aria-hidden="true"><i></i><i></i><i></i></span>
                <div><strong>{move || if planning.get() { "方案规划运行中" } else { "需求分析运行中" }}</strong><small>{move || agent_activity.get()}</small></div>
                <button class="button button-quiet" on:click=move |_| on_cancel.run(())>
                    {move || if planning.get() { "停止规划" } else { "停止分析" }}
                </button>
            </div>
            <p class="analysis-progress-prompt">{move || prompt.get()}</p>
            <div class="analysis-progress-steps">
                <Show
                    when=move || planning.get()
                    fallback=move || view! {
                        <div class=move || { if orchestration_stage.get() > 0 { "orchestration-stage is-complete" } else { "orchestration-stage is-active" } }>
                            <b>"01"</b><strong>"风险路由"</strong><small>"协调器"</small>
                        </div>
                        <div class=move || { if orchestration_stage.get() > 1 { "orchestration-stage is-complete" } else if orchestration_stage.get() == 1 { "orchestration-stage is-active" } else { "orchestration-stage" } }>
                            <b>"02"</b><strong>"需求分析"</strong><small>"单模型 / 多模型投票"</small>
                        </div>
                        <div class=move || { if orchestration_stage.get() > 2 { "orchestration-stage is-complete" } else if orchestration_stage.get() == 2 { "orchestration-stage is-active" } else { "orchestration-stage" } }>
                            <b>"03"</b><strong>"业务模式"</strong><small>"启发式识别器"</small>
                        </div>
                        <div class=move || { if orchestration_stage.get() > 3 { "orchestration-stage is-complete" } else if orchestration_stage.get() == 3 { "orchestration-stage is-active" } else { "orchestration-stage" } }>
                            <b>"04"</b><strong>"澄清排序"</strong><small>"问题优先级"</small>
                        </div>
                        <div class=move || { if orchestration_stage.get() > 4 { "orchestration-stage is-complete" } else if orchestration_stage.get() == 4 { "orchestration-stage is-active" } else { "orchestration-stage" } }>
                            <b>"05"</b><strong>"保存结果"</strong><small>"工作流协调器"</small>
                        </div>
                    }
                >
                    <div class="orchestration-stage is-active">
                        <b>"01"</b><strong>"方案规划与校验"</strong><small>"规划智能体"</small>
                    </div>
                </Show>
            </div>
            <Show when=move || !orchestration_events.get().is_empty()>
                <ol class="analysis-progress-events" aria-label="编排事件轨迹">
                    <For
                        each=move || orchestration_events.get()
                        key=|event| event.clone()
                        let:item
                    >
                        <li>{item}</li>
                    </For>
                </ol>
            </Show>
        </section>
    }
}

#[component]
pub fn DecisionPlanView(plan: DecisionPlan, requirements: Vec<RequirementItem>) -> impl IntoView {
    let decisions = RwSignal::new(plan.decisions);
    let requirement_lookup = requirements
        .into_iter()
        .map(|item| (item.id, item.text))
        .collect::<std::collections::HashMap<_, _>>();
    let knowledge_models = RwSignal::new(plan.business_knowledge_models);
    let services = RwSignal::new(plan.decision_services);
    let defaults = RwSignal::new(plan.defaults);
    let criteria = RwSignal::new(plan.acceptance_criteria);
    view! {
        <div class="decision-plan-view">
            <p>{plan.summary}</p>
            <div class="plan-decision-list">
                <For each=move || decisions.get() key=|decision| decision.name.clone() let:decision>
                    {
                        let links = decision.requirement_ids.iter().filter_map(|id| {
                            requirement_lookup.get(id).map(|text| format!("{id}：{text}"))
                        }).collect::<Vec<_>>().join("；");
                        view! { <div>
                            <strong>{decision.name}</strong><span>{decision.expression_type}</span>
                            <small>{decision.purpose}</small><code>{decision.dependencies.join(" → ")}</code>
                            <small class="plan-requirement-links">{if links.is_empty() { "未记录需求关联".to_string() } else { format!("需求依据：{links}") }}</small>
                        </div> }
                    }
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

fn tester_category_label(category: &str) -> &'static str {
    match category {
        "required_input" => "必填输入",
        "optional_input" => "可选输入",
        "clarification" => "业务澄清",
        "semantic_requirement" | "semantic_scenario" => "业务语义场景",
        "static_analysis" => "模型静态发现",
        "acceptance_criterion" => "验收标准",
        "decision_contract" => "决策契约",
        "adversarial" => "对抗场景",
        _ => "基础场景",
    }
}

fn tester_proposal_group(category: &str) -> &'static str {
    match category {
        "static_analysis" => "model",
        "acceptance_criterion" | "decision_contract" | "adversarial" => "rules",
        _ => "business",
    }
}

fn tester_proposal_group_label(group: &str) -> &'static str {
    match group {
        "business" => "业务场景",
        "model" => "模型发现",
        "rules" => "规则与边界",
        _ => "全部提案",
    }
}

fn tester_proposal_matches_version(
    proposal: &SemanticTestProposal,
    version_id: Option<i64>,
) -> bool {
    if proposal.category != "static_analysis" {
        return true;
    }
    let Some(version_id) = version_id else {
        return false;
    };
    proposal
        .source_reference
        .strip_prefix("draftVersion:")
        .and_then(|source| source.split_once('/'))
        .and_then(|(source_version, _)| source_version.parse::<i64>().ok())
        == Some(version_id)
}

#[component]
pub fn TesterReportView(
    report: TesterReport,
    project_id: Option<i64>,
    version_id: Option<i64>,
    token: Option<String>,
    can_model: Signal<bool>,
    on_converted: Callback<DecisionTestCase>,
) -> impl IntoView {
    let proposals = RwSignal::new(
        report
            .proposals
            .into_iter()
            .filter(|proposal| tester_proposal_matches_version(proposal, version_id))
            .collect::<Vec<_>>(),
    );
    let (run_busy, set_run_busy) = signal(false);
    let (run_message, set_run_message) = signal(None::<String>);
    let (active_group, set_active_group) = signal("all".to_string());
    let proposal_token = StoredValue::new(token.clone());
    let run_token = token.clone();
    let run_cases = Callback::new(move |_| {
        let (Some(project_id), Some(version_id), Some(token)) =
            (project_id, version_id, run_token.clone())
        else {
            set_run_message.set(Some("请先切换到当前可编辑版本".into()));
            return;
        };
        if run_busy.get()
            || !proposals
                .get()
                .iter()
                .any(|proposal| proposal.test_case_id.is_some())
        {
            return;
        }
        set_run_busy.set(true);
        set_run_message.set(None);
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::run_tester_proposal_cases(&token, project_id, version_id).await {
                Ok(cases) => {
                    let passed = cases.iter().filter(|case| case.status == "passed").count();
                    let failed = cases.iter().filter(|case| case.status == "failed").count();
                    let errors = cases.iter().filter(|case| case.status == "error").count();
                    proposals.update(|items| {
                        for item in items {
                            if let Some(case_id) = item.test_case_id {
                                if let Some(case) = cases.iter().find(|case| case.id == case_id) {
                                    item.execution_status = Some(case.status.clone());
                                }
                            }
                        }
                    });
                    set_run_message.set(Some(format!(
                        "Tester 用例执行完成：{} 个，通过 {}，不一致 {}，错误 {}",
                        cases.len(),
                        passed,
                        failed,
                        errors
                    )));
                }
                Err(message) => set_run_message.set(Some(message)),
            }
            set_run_busy.set(false);
        });
    });
    let on_proposal_converted = Callback::new(
        move |(proposal_id, test_case): (String, DecisionTestCase)| {
            proposals.update(|items| {
                if let Some(item) = items.iter_mut().find(|item| item.id == proposal_id) {
                    item.test_case_id = Some(test_case.id);
                }
            });
            on_converted.run(test_case);
        },
    );
    view! {
        <section class="tester-report" aria-label="测试工程提案">
            <div class="tester-report-heading">
                <h3>"测试工程提案"</h3>
                <div class="tester-report-actions">
                    <span>{move || {
                        let pending = proposals
                            .get()
                            .iter()
                            .filter(|proposal| proposal.test_case_id.is_none())
                            .count();
                        format!("{} 条待复核", pending)
                    }}</span>
                    <button
                        class="button button-quiet"
                        on:click=move |_| run_cases.run(())
                        disabled=move || run_busy.get()
                            || project_id.is_none()
                            || version_id.is_none()
                            || !proposals.get().iter().any(|proposal| proposal.test_case_id.is_some())
                    >
                        {move || if run_busy.get() { "正在执行..." } else { "运行已转用例" }}
                    </button>
                </div>
            </div>
            <Show when=move || run_message.get().is_some()>
                <p class="tester-execution-status" role="status">{move || run_message.get().unwrap_or_default()}</p>
            </Show>
            <p>"提案仅用于补充覆盖范围；期望结果由业务人员确认后录入正式测试用例。"</p>
            <div class="tester-proposal-tabs" role="tablist" aria-label="按提案类型筛选">
                <button
                    type="button"
                    role="tab"
                    id="tester-proposal-tab-all"
                    aria-controls="tester-proposal-panel"
                    aria-selected=move || active_group.get() == "all"
                    class=move || if active_group.get() == "all" { "tester-proposal-tab is-active" } else { "tester-proposal-tab" }
                    on:click=move |_| set_active_group.set("all".into())
                >
                    <span>"全部提案"</span>
                    <b>{move || proposals.get().len()}</b>
                </button>
                <For
                    each=move || {
                        ["business", "model", "rules"]
                        .into_iter()
                        .filter(|group| proposals.get().iter().any(|item| tester_proposal_group(&item.category) == *group))
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                    }
                    key=|group| group.clone()
                    let:group
                >
                    {
                        let tab_id = format!("tester-proposal-tab-{group}");
                        let tab_label = tester_proposal_group_label(&group).to_string();
                        let selected_key = group.clone();
                        let class_key = group.clone();
                        let count_key = group.clone();
                        let tab_key = group;
                        view! {
                            <button
                                type="button"
                                role="tab"
                                id=tab_id
                                aria-controls="tester-proposal-panel"
                                aria-selected=move || active_group.get() == selected_key
                                class=move || if active_group.get() == class_key { "tester-proposal-tab is-active" } else { "tester-proposal-tab" }
                                on:click=move |_| set_active_group.set(tab_key.clone())
                            >
                                <span>{tab_label}</span>
                                <b>{move || proposals.get().iter().filter(|item| tester_proposal_group(&item.category) == count_key).count()}</b>
                            </button>
                        }
                    }
                </For>
            </div>
            <div
                id="tester-proposal-panel"
                class="tester-proposal-list"
                role="tabpanel"
                aria-labelledby=move || format!("tester-proposal-tab-{}", active_group.get())
            >
                <Show
                    when=move || !proposals.get().is_empty()
                    fallback=|| view! { <p class="tester-proposal-empty">"当前版本没有可展示的测试提案。"</p> }
                >
                    <For
                        each=move || {
                            proposals.get().into_iter().filter(|item| {
                                active_group.get() == "all" || tester_proposal_group(&item.category) == active_group.get()
                            }).collect::<Vec<_>>()
                        }
                        key=|item| item.id.clone()
                        let:item
                    >
                        <TesterProposalRow
                            item=item
                            project_id=project_id
                            version_id=version_id
                            token=proposal_token.get_value()
                            can_model=can_model
                            on_proposal_converted=on_proposal_converted
                        />
                    </For>
                </Show>
            </div>
        </section>
    }
}

#[component]
fn TesterProposalRow(
    item: SemanticTestProposal,
    project_id: Option<i64>,
    version_id: Option<i64>,
    token: Option<String>,
    can_model: Signal<bool>,
    on_proposal_converted: Callback<(String, DecisionTestCase)>,
) -> impl IntoView {
    let input_fields = item.input_fields.join("、");
    let has_input_fields = !input_fields.is_empty();
    let proposal_id = item.id.clone();
    let proposal_case_id = item.test_case_id;
    let execution_label = item.execution_status.as_deref().map(|status| match status {
        "passed" => "DMN 执行通过",
        "failed" => "DMN 执行不一致",
        "error" => "DMN 执行错误",
        _ => "DMN 尚未执行",
    });
    let proposal_name = item.title.clone();
    let initial_input = item
        .suggested_input
        .as_ref()
        .and_then(|input| serde_json::to_string_pretty(input).ok())
        .unwrap_or_else(|| "{}".into());
    let has_suggested_input = item.suggested_input.is_some();
    let (input_json, set_input_json) = signal(initial_input);
    let (expected_output_json, set_expected_output_json) = signal("{}".to_string());
    let (busy, set_busy) = signal(false);
    let (error, set_error) = signal(None::<String>);
    let (converted_case, set_converted_case) = signal(None::<DecisionTestCase>);
    let (judge_review, set_judge_review) = signal(item.judge_review.clone());
    let (judge_stale, set_judge_stale) = signal(item.judge_review.is_some());
    let judge_proposal_name = proposal_name.clone();
    let judge_token = token.clone();
    let judge_proposal_id = proposal_id.clone();
    let judge = Callback::<()>::new(move |_| {
        let (Some(project_id), Some(version_id), Some(token)) =
            (project_id, version_id, judge_token.clone())
        else {
            set_error.set(Some("请先切换到当前可编辑版本".into()));
            return;
        };
        let input_json = match serde_json::from_str::<serde_json::Value>(&input_json.get()) {
            Ok(value) if value.is_object() => value,
            Ok(_) => {
                set_error.set(Some("测试输入必须是 JSON 对象".into()));
                return;
            }
            Err(error) => {
                set_error.set(Some(format!("输入 JSON 无效：{error}")));
                return;
            }
        };
        let expected_output_json =
            match serde_json::from_str::<serde_json::Value>(&expected_output_json.get()) {
                Ok(value) if value.as_object().is_some_and(|object| !object.is_empty()) => value,
                Ok(_) => {
                    set_error.set(Some(
                        "期望输出必须是至少包含一个决策断言的 JSON 对象".into(),
                    ));
                    return;
                }
                Err(error) => {
                    set_error.set(Some(format!("期望输出 JSON 无效：{error}")));
                    return;
                }
            };
        let token_for_request = token.clone();
        let proposal_id = judge_proposal_id.clone();
        let proposal_name = judge_proposal_name.clone();
        set_busy.set(true);
        set_error.set(None);
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::judge_tester_proposal(
                &token_for_request,
                project_id,
                version_id,
                &proposal_id,
                CreateDecisionTestCase {
                    name: proposal_name,
                    input_json,
                    expected_output_json,
                },
            )
            .await
            {
                Ok(review) => {
                    set_judge_review.set(Some(review));
                    set_judge_stale.set(false);
                }
                Err(message) => set_error.set(Some(message)),
            }
            set_busy.set(false);
        });
    });
    let convert = Callback::<()>::new(move |_| {
        let (Some(project_id), Some(version_id), Some(token)) =
            (project_id, version_id, token.clone())
        else {
            set_error.set(Some("请先切换到当前可编辑版本".into()));
            return;
        };
        let input_json = match serde_json::from_str::<serde_json::Value>(&input_json.get()) {
            Ok(value) => value,
            Err(error) => {
                set_error.set(Some(format!("输入 JSON 无效：{error}")));
                return;
            }
        };
        let expected_output_json =
            match serde_json::from_str::<serde_json::Value>(&expected_output_json.get()) {
                Ok(value) => value,
                Err(error) => {
                    set_error.set(Some(format!("期望输出 JSON 无效：{error}")));
                    return;
                }
            };
        let token_for_request = token.clone();
        let proposal_id = proposal_id.clone();
        let proposal_id_for_callback = proposal_id.clone();
        let proposal_name = proposal_name.clone();
        set_busy.set(true);
        set_error.set(None);
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::convert_tester_proposal(
                &token_for_request,
                project_id,
                version_id,
                &proposal_id,
                CreateDecisionTestCase {
                    name: proposal_name,
                    input_json,
                    expected_output_json,
                },
            )
            .await
            {
                Ok(test_case) => {
                    set_converted_case.set(Some(test_case.clone()));
                    on_proposal_converted.run((proposal_id_for_callback, test_case));
                }
                Err(message) => set_error.set(Some(message)),
            }
            set_busy.set(false);
        });
    });
    view! {
        <article class="tester-proposal">
            <div class="tester-proposal-title">
                <strong>{item.title}</strong>
                <span>{tester_category_label(&item.category)}</span>
            </div>
            <p>{item.rationale}</p>
            <small>{format!("依据：{}", item.source_reference)}</small>
            <Show when=move || has_input_fields>
                <small>{format!("关联输入：{}", input_fields)}</small>
            </Show>
            {move || converted_case.get().map(|test_case| test_case.id).or(proposal_case_id).map(|case_id| view! {
                <small class="tester-proposal-converted">{format!("已转为待执行用例 #{}", case_id)}</small>
            })}
            {execution_label.map(|label| view! { <small class="tester-proposal-execution">{label}</small> })}
            <Show when=move || converted_case.get().is_none() && proposal_case_id.is_none()>
                <details class="tester-proposal-convert">
                    <summary>"填写输入与期望输出"</summary>
                    <label>
                        <span>"测试输入 JSON"</span>
                        <Show when=move || has_suggested_input>
                            <small>"按当前规则条件生成的参考输入，请核对后使用。"</small>
                        </Show>
                        <textarea
                            prop:value=move || input_json.get()
                            on:input=move |event| {
                                set_input_json.set(event_target_value(&event));
                                set_judge_stale.set(true);
                            }
                            disabled=move || busy.get() || project_id.is_none() || version_id.is_none() || !can_model.get()
                        ></textarea>
                    </label>
                    <label>
                        <span>"业务确认的期望输出 JSON"</span>
                        <textarea
                            prop:value=move || expected_output_json.get()
                            on:input=move |event| {
                                set_expected_output_json.set(event_target_value(&event));
                                set_judge_stale.set(true);
                            }
                            disabled=move || busy.get() || project_id.is_none() || version_id.is_none() || !can_model.get()
                        ></textarea>
                    </label>
                    <Show when=move || judge_review.get().is_some()>
                        {move || judge_review.get().map(|review| tester_judge_review_view(review, judge_stale.get()))}
                    </Show>
                    <Show when=move || error.get().is_some()>
                        <small class="tester-proposal-error">{move || error.get().unwrap_or_default()}</small>
                    </Show>
                    <Show when=move || can_model.get()>
                        <button
                            class="button button-secondary"
                            on:click=move |_| judge.run(())
                            disabled=move || busy.get() || project_id.is_none() || version_id.is_none()
                        >
                            {move || if busy.get() { "复核处理中..." } else { "AI 建议复核" }}
                        </button>
                    </Show>
                    <button
                        class="button button-secondary"
                        on:click=move |_| convert.run(())
                        disabled=move || busy.get() || project_id.is_none() || version_id.is_none() || !can_model.get()
                    >
                        {move || if busy.get() { "正在保存..." } else { "保存为待执行用例" }}
                    </button>
                </details>
            </Show>
        </article>
    }
}

fn tester_judge_review_view(review: TesterJudgeReview, stale: bool) -> impl IntoView {
    let status = match review.status.as_str() {
        "no_obvious_issue" => "两次判定均未发现明显问题",
        "suspicious" => "两次判定均提示可疑",
        _ => "判定不一致或证据不足",
    };
    let stale_label = if stale {
        "内容可能已变化，此结果已过期；请重新复核"
    } else {
        "最近一次复核结果（仅供参考，不代表批准）"
    };
    view! {
        <div class="tester-judge-review" role="status">
            <strong>{status}</strong>
            <small>{stale_label}</small>
            <small>{review.summary}</small>
            <For each=move || review.votes.clone() key=|vote| format!("{}:{}", vote.assessment, vote.reason) let:vote>
                <small>{format!("{}：{}", match vote.assessment.as_str() { "plausible" => "依据充分", "suspicious" => "发现疑点", _ => "证据不足" }, vote.reason)}</small>
            </For>
        </div>
    }
}
