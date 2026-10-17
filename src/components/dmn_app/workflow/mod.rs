mod presentation;

use leptos::prelude::*;
use std::collections::BTreeMap;

use crate::components::common::{SelectOption, WorkbenchSelect};
use crate::workflow::{Clarification, DecisionBrief, DecisionField, DecisionPlan};
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
pub fn DecisionPlanView(plan: DecisionPlan) -> impl IntoView {
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
