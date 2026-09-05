use leptos::prelude::*;

use crate::models::ExecutionTrace;

#[component]
pub fn ExecutionTraceView(trace: ExecutionTrace) -> impl IntoView {
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
