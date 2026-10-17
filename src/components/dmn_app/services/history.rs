use leptos::prelude::*;

use crate::components::common::{SelectOption, WorkbenchSelect};
use crate::components::dmn_app::execution::ExecutionTraceView;
use crate::components::workspace::{outcome_assessment_label, recent_service_runs};
use crate::models::{DecisionServiceRun, ExecutionTrace};

#[component]
pub fn ServiceRunHistory(
    service_runs: ReadSignal<Vec<DecisionServiceRun>>,
    outcome_run_id: ReadSignal<Option<i64>>,
    set_outcome_run_id: WriteSignal<Option<i64>>,
    outcome_business_reference: ReadSignal<String>,
    set_outcome_business_reference: WriteSignal<String>,
    outcome_actual_json: ReadSignal<String>,
    set_outcome_actual_json: WriteSignal<String>,
    outcome_assessment: ReadSignal<String>,
    set_outcome_assessment: WriteSignal<String>,
    outcome_note: ReadSignal<String>,
    set_outcome_note: WriteSignal<String>,
    outcome_occurred_at: ReadSignal<String>,
    set_outcome_occurred_at: WriteSignal<String>,
    outcome_busy: ReadSignal<bool>,
    on_edit_run: Callback<DecisionServiceRun>,
    on_save_outcome: Callback<web_sys::MouseEvent>,
) -> impl IntoView {
    view! {
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
                                        <button class="button button-quiet outcome-edit-button" on:click=move |_| on_edit_run.run(run_for_edit.get())>
                                            {if has_assessment { "修改实际结果" } else { "回填实际结果" }}
                                        </button>
                                    </Show>
                                    <Show when=move || outcome_run_id.get() == Some(run_id)>
                                        <div class="outcome-editor">
                                            <div class="outcome-editor-grid">
                                                <label><span>"业务关联号"</span><input maxlength="255" prop:value=outcome_business_reference on:input=move |event| set_outcome_business_reference.set(event_target_value(&event)) /></label>
                                                <label><span>"结果发生时间"</span><input type="datetime-local" prop:value=outcome_occurred_at on:input=move |event| set_outcome_occurred_at.set(event_target_value(&event)) /></label>
                                                <WorkbenchSelect
                                                    label="结果判定"
                                                    options=Signal::derive(|| vec![SelectOption::new("confirmed", "符合决策"), SelectOption::new("mismatch", "存在偏差"), SelectOption::new("inconclusive", "无法判断")])
                                                    value=outcome_assessment
                                                    disabled=outcome_busy
                                                    placeholder=Signal::derive(|| "请选择结果判定".to_string())
                                                    on_change=Callback::new(move |value| set_outcome_assessment.set(value))
                                                />
                                                <label class="outcome-editor-wide"><span>"实际业务结果 JSON"</span><textarea rows="4" spellcheck="false" prop:value=outcome_actual_json on:input=move |event| set_outcome_actual_json.set(event_target_value(&event))></textarea></label>
                                                <label class="outcome-editor-wide"><span>"反馈说明"</span><textarea rows="2" maxlength="4000" prop:value=outcome_note on:input=move |event| set_outcome_note.set(event_target_value(&event))></textarea></label>
                                            </div>
                                            <div class="outcome-editor-actions">
                                                <button class="button button-quiet" on:click=move |_| set_outcome_run_id.set(None) disabled=move || outcome_busy.get()>"取消"</button>
                                                <button class="button button-primary" on:click=move |event| on_save_outcome.run(event) disabled=move || outcome_busy.get() || outcome_business_reference.get().trim().is_empty()>
                                                    {move || if outcome_busy.get() { "保存中..." } else { "保存实际结果" }}
                                                </button>
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
    }
}
