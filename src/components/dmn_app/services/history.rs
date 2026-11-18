use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use crate::components::common::{SelectOption, WorkbenchSelect};
use crate::components::dmn_app::execution::ExecutionTraceView;
use crate::components::workspace::{outcome_assessment_label, recent_service_runs};
use crate::models::{
    DecisionRunExplanationReviewView, DecisionRunExplanationView, DecisionServiceRun,
    ExecutionTrace,
};

#[component]
pub fn ServiceRunHistory(
    service_runs: ReadSignal<Vec<DecisionServiceRun>>,
    project_id: Signal<Option<i64>>,
    token: Signal<Option<String>>,
    reviewer_username: Signal<String>,
    can_model: Signal<bool>,
    can_review: Signal<bool>,
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
                            let explanation = RwSignal::new(None::<DecisionRunExplanationView>);
                            let explanation_busy = RwSignal::new(false);
                            let explanation_error = RwSignal::new(None::<String>);
                            let explanation_reviews =
                                RwSignal::new(Vec::<DecisionRunExplanationReviewView>::new());
                            let review_label = RwSignal::new(String::new());
                            let review_busy = RwSignal::new(false);
                            let review_submitted = RwSignal::new(false);
                            let review_notice = RwSignal::new(None::<String>);
                            let explanation_project = project_id;
                            let explanation_token = token;
                            let explanation_can_model = can_model;
                            let explanation_can_review = can_review;
                            let explanation_reviewer_username = reviewer_username;
                            let load_explanation = move |_| {
                                if explanation_busy.get_untracked() || review_busy.get_untracked() {
                                    return;
                                }
                                let (Some(project), Some(auth_token)) =
                                    (explanation_project.get_untracked(), explanation_token.get_untracked())
                                else {
                                    explanation_error.set(Some("请重新登录后再试".into()));
                                    return;
                                };
                                let generate = explanation_can_model.get_untracked();
                                explanation_busy.set(true);
                                explanation_error.set(None);
                                explanation_reviews.set(Vec::new());
                                review_submitted.set(false);
                                review_label.set(String::new());
                                review_notice.set(None);
                                spawn_local(async move {
                                    let mut current = if generate {
                                        match crate::api::generate_decision_run_explanation(&auth_token, project, run_id).await {
                                            Ok(view) => Ok(view),
                                            Err(post_error) => match crate::api::get_decision_run_explanation(&auth_token, project, run_id).await {
                                                Ok(view) if matches!(view.status.as_str(), "pending" | "running" | "succeeded") => Ok(view),
                                                _ => Err(post_error),
                                            },
                                        }
                                    } else {
                                        crate::api::get_decision_run_explanation(&auth_token, project, run_id).await
                                    };
                                    let context_is_current = || {
                                        explanation_project.get_untracked() == Some(project)
                                            && explanation_token.get_untracked().as_deref() == Some(auth_token.as_str())
                                    };
                                    if let Ok(view) = current.as_ref()
                                        && context_is_current()
                                    {
                                        explanation.set(Some(view.clone()));
                                    }
                                    for _ in 0..30 {
                                        let Ok(view) = current.as_ref() else { break };
                                        if !matches!(view.status.as_str(), "pending" | "running") || !context_is_current() {
                                            break;
                                        }
                                        let promise = js_sys::Promise::new(&mut |resolve, _| {
                                            if let Some(window) = web_sys::window() {
                                                let callback = wasm_bindgen::closure::Closure::once_into_js(move || {
                                                    let _ = resolve.call0(&JsValue::NULL);
                                                });
                                                let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                                                    callback.unchecked_ref(),
                                                    2_000,
                                                );
                                            }
                                        });
                                        let _ = JsFuture::from(promise).await;
                                        if !context_is_current() { break; }
                                        current = crate::api::get_decision_run_explanation(&auth_token, project, run_id).await;
                                        match current.as_ref() {
                                            Ok(view) => explanation.set(Some(view.clone())),
                                            Err(_) => break,
                                        }
                                    }
                                    if context_is_current() {
                                        match current {
                                            Err(message) => explanation_error.set(Some(message)),
                                            Ok(view) if view.status == "failed" => {
                                                explanation_error.set(Some(
                                                    view.error_message.unwrap_or_else(|| "解释生成失败".into()),
                                                ));
                                            }
                                            Ok(view) if matches!(view.status.as_str(), "pending" | "running") => {
                                                explanation_error.set(Some("解释仍在处理中，可稍后重新查看".into()));
                                            }
                                            _ => {}
                                        }
                                        if let Some(view) = explanation.get_untracked().filter(|view| view.status == "succeeded")
                                            && let Ok(reviews) = crate::api::list_decision_run_explanation_reviews(&auth_token, project, run_id, view.generation_revision).await
                                            && context_is_current()
                                        {
                                            let username = explanation_reviewer_username.get_untracked();
                                            review_submitted.set(reviews.iter().any(|review| review.reviewer_username == username));
                                            explanation_reviews.set(reviews);
                                        }
                                        explanation_busy.set(false);
                                    }
                                });
                            };
                            let submit_review = move |_| {
                                if review_busy.get_untracked() || explanation_busy.get_untracked() || !explanation_can_review.get_untracked() {
                                    return;
                                }
                                let (Some(project), Some(auth_token)) =
                                    (explanation_project.get_untracked(), explanation_token.get_untracked())
                                else {
                                    review_notice.set(Some("请重新登录后再试".into()));
                                    return;
                                };
                                let Some(view) = explanation.get_untracked().filter(|view| view.status == "succeeded") else {
                                    return;
                                };
                                let reviewed_revision = view.generation_revision;
                                let label = review_label.get_untracked();
                                review_busy.set(true);
                                review_notice.set(None);
                                spawn_local(async move {
                                    match crate::api::review_decision_run_explanation(&auth_token, project, run_id, reviewed_revision, &label).await {
                                        Ok(_) if explanation_project.get_untracked() == Some(project)
                                            && explanation_token.get_untracked().as_deref() == Some(auth_token.as_str())
                                            && explanation.get_untracked().is_some_and(|view| view.generation_revision == reviewed_revision) => {
                                            review_submitted.set(true);
                                            match crate::api::list_decision_run_explanation_reviews(&auth_token, project, run_id, reviewed_revision).await {
                                                Ok(reviews)
                                                    if explanation_project.get_untracked() == Some(project)
                                                        && explanation_token.get_untracked().as_deref() == Some(auth_token.as_str())
                                                        && explanation.get_untracked().is_some_and(|view| view.generation_revision == reviewed_revision) => {
                                                    explanation_reviews.set(reviews)
                                                }
                                                Err(_) => {}
                                                _ => {}
                                            }
                                            review_notice.set(Some("审阅已记录".into()));
                                        }
                                        Ok(_) => {}
                                        Err(message)
                                            if explanation_project.get_untracked() == Some(project)
                                                && explanation_token.get_untracked().as_deref() == Some(auth_token.as_str())
                                                && explanation.get_untracked().is_some_and(|view| view.generation_revision == reviewed_revision) => {
                                            review_notice.set(Some(message))
                                        }
                                        Err(_) => {}
                                    }
                                    if explanation_project.get_untracked() == Some(project)
                                        && explanation_token.get_untracked().as_deref() == Some(auth_token.as_str())
                                    {
                                        review_busy.set(false);
                                    }
                                });
                            };
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
                                    <Show when=move || can_edit_outcome>
                                        <div class="decision-explanation-control">
                                            <button class="button button-quiet" on:click=load_explanation disabled=move || explanation_busy.get() || review_busy.get()>
                                                {move || if explanation_busy.get() { "解释处理中..." } else if can_model.get() { "生成/查看解释" } else { "查看解释" }}
                                            </button>
                                            <Show when=move || explanation_error.get().is_some()>
                                                <span class="decision-explanation-error" role="status">{move || explanation_error.get().unwrap_or_default()}</span>
                                            </Show>
                                        </div>
                                    </Show>
                                    <Show when=move || explanation.get().and_then(|view| view.explanation_json.as_ref().and_then(|json| json.get("summary").and_then(serde_json::Value::as_str).map(str::to_string))).is_some()>
                                        <div class="decision-explanation">
                                            <strong>"决策解释"</strong>
                                            <p>{move || explanation.get().and_then(|view| view.explanation_json.as_ref().and_then(|json| json.get("summary").and_then(serde_json::Value::as_str).map(str::to_string))).unwrap_or_default()}</p>
                                            <For
                                                each=move || explanation.get().and_then(|view| view.explanation_json.and_then(|json| json.get("decisions").and_then(serde_json::Value::as_array).cloned())).unwrap_or_default()
                                                key=|step| step.get("decisionId").and_then(serde_json::Value::as_str).unwrap_or_default().to_string()
                                                let:step
                                            >
                                                <div class="decision-explanation-step">
                                                    <span>{step.get("decisionId").and_then(serde_json::Value::as_str).map(str::to_owned).unwrap_or_else(|| "决策步骤".into())}</span>
                                                    <p>{step.get("explanation").and_then(serde_json::Value::as_str).map(str::to_owned).unwrap_or_default()}</p>
                                                </div>
                                            </For>
                                            <Show when=move || can_review.get() && !review_submitted.get()>
                                                <div class="decision-explanation-review">
                                                    <WorkbenchSelect
                                                        label="可读性审阅"
                                                        options=Signal::derive(|| vec![
                                                            SelectOption::new("readable", "清晰可读"),
                                                            SelectOption::new("needs_revision", "需要改进"),
                                                            SelectOption::new("inconclusive", "无法判断"),
                                                        ])
                                                        value=Signal::derive(move || review_label.get())
                                                        disabled=Signal::derive(move || review_busy.get() || explanation_busy.get())
                                                        placeholder=Signal::derive(|| "选择审阅结论".to_string())
                                                        on_change=Callback::new(move |value| review_label.set(value))
                                                    />
                                                    <button class="button button-quiet" on:click=submit_review disabled=move || review_busy.get() || explanation_busy.get() || review_label.get().is_empty()>
                                                        {move || if review_busy.get() { "提交中..." } else { "记录审阅" }}
                                                    </button>
                                                    <Show when=move || review_notice.get().is_some()>
                                                        <span class="decision-explanation-review-notice" role="status">{move || review_notice.get().unwrap_or_default()}</span>
                                                    </Show>
                                                </div>
                                            </Show>
                                            <Show when=move || !explanation_reviews.get().is_empty() && (!can_review.get() || review_submitted.get())>
                                                <div class="decision-explanation-review-history">
                                                    <strong>{move || format!("人工审阅 {} 条", explanation_reviews.get().len())}</strong>
                                                    <For each=move || explanation_reviews.get() key=|review| review.id let:review>
                                                        <span>{format!("{} · {} · {}", match review.review_label.as_str() { "readable" => "清晰可读", "needs_revision" => "需要改进", _ => "无法判断" }, review.reviewer_username, review.created_at.format("%Y-%m-%d %H:%M"))}</span>
                                                    </For>
                                                </div>
                                            </Show>
                                        </div>
                                    </Show>
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
