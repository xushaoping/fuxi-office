use leptos::prelude::*;
use std::collections::BTreeMap;

use crate::workflow::{Clarification, DecisionBrief, DecisionField, DecisionPlan};

#[component]
pub fn ClarificationInput(
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
pub fn WorkflowFieldRow(field: DecisionField) -> impl IntoView {
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
pub fn DecisionBriefView(brief: DecisionBrief) -> impl IntoView {
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
