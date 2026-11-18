use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
use std::{cell::RefCell, collections::HashMap};
use wasm_bindgen::{JsCast, closure::Closure};

thread_local! {
    static PROMPT_EVALUATION_POLLERS: RefCell<HashMap<i32, Closure<dyn FnMut()>>> = RefCell::new(HashMap::new());
}

use crate::api;
use crate::components::auth::auth_token;
use crate::components::common::{SelectOption, WorkbenchNumberInput, WorkbenchSelect};
use crate::models::{CreatePromptDraftRequest, PromptEvaluationJobView, PromptEvaluationRequest};

fn prompt_key_label(key: &str) -> &'static str {
    match key {
        "requirement_analysis" => "需求分析",
        "decision_planning" => "方案规划",
        "dsl_generation" => "DSL 生成",
        "change_intent" => "变更意图",
        "optimization_proposal" => "优化建议",
        "decision_explanation" => "决策解释",
        "tester_judge" => "Tester 期望值复核",
        _ => "未知类别",
    }
}

fn prompt_status_label(status: &str) -> &'static str {
    match status {
        "draft" => "草稿",
        "evaluated" => "评测通过",
        "active" => "当前生效",
        "retired" => "已退役",
        _ => "未知状态",
    }
}

fn prompt_key_options() -> Vec<SelectOption> {
    vec![
        SelectOption::new("requirement_analysis", "需求分析"),
        SelectOption::new("decision_planning", "方案规划"),
        SelectOption::new("dsl_generation", "DSL 生成"),
        SelectOption::new("change_intent", "变更意图"),
        SelectOption::new("optimization_proposal", "优化建议"),
        SelectOption::new("decision_explanation", "决策解释"),
        SelectOption::new("tester_judge", "Tester 期望值复核"),
    ]
}

#[component]
pub fn AdminPromptRegistryPage() -> impl IntoView {
    let token = auth_token();
    let navigate = use_navigate();
    let navigate_to_users = navigate.clone();
    let navigate_to_intelligence = navigate.clone();
    let assets = RwSignal::new(Vec::<crate::models::PromptAssetAdminView>::new());
    let events = RwSignal::new(Vec::<crate::models::PromptAssetEventAdminView>::new());
    let prompt_key = RwSignal::new("requirement_analysis".to_string());
    let version = RwSignal::new(String::new());
    let content = RwSignal::new(String::new());
    let dataset_hash = RwSignal::new(String::new());
    let case_count = RwSignal::new(String::new());
    let report = RwSignal::new("{}".to_string());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let notice = RwSignal::new(None::<String>);
    let evaluation_job = RwSignal::new(None::<PromptEvaluationJobView>);

    let reload = move || {
        let Some(token_value) = token.get() else {
            return;
        };
        spawn_local(async move {
            match api::list_prompt_assets(&token_value).await {
                Ok(value) => assets.set(value),
                Err(message) => error.set(Some(message)),
            }
            match api::list_prompt_asset_events(&token_value).await {
                Ok(value) => events.set(value),
                Err(message) => error.set(Some(message)),
            }
            match api::list_prompt_evaluation_jobs(&token_value).await {
                Ok(mut jobs) => evaluation_job.set((!jobs.is_empty()).then(|| jobs.remove(0))),
                Err(message) => error.set(Some(message)),
            }
        });
    };

    Effect::new(move |_| {
        if token.get().is_some() {
            reload();
        }
    });

    let create_draft = move |_| {
        let Some(token_value) = token.get() else {
            return;
        };
        if busy.get() {
            return;
        }
        busy.set(true);
        error.set(None);
        notice.set(None);
        let payload = CreatePromptDraftRequest {
            prompt_key: prompt_key.get(),
            version: version.get().trim().to_string(),
            content: content.get(),
        };
        spawn_local(async move {
            match api::create_prompt_draft(&token_value, payload).await {
                Ok(asset) => {
                    version.set(String::new());
                    content.set(String::new());
                    notice.set(Some(format!(
                        "{} {} 草稿已创建",
                        prompt_key_label(&asset.prompt_key),
                        asset.version
                    )));
                    reload();
                }
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    let record_evaluation = move |asset_id: i64| {
        let Some(token_value) = token.get() else {
            return;
        };
        let Ok(case_count_value) = case_count.get().trim().parse::<u32>() else {
            error.set(Some("评测样本数须为正整数".into()));
            return;
        };
        let report_value = match serde_json::from_str::<serde_json::Value>(&report.get()) {
            Ok(value) if value.is_object() => value,
            _ => {
                error.set(Some("评测报告须为 JSON 对象".into()));
                return;
            }
        };
        if busy.get() {
            return;
        }
        busy.set(true);
        error.set(None);
        let payload = PromptEvaluationRequest {
            dataset_hash: dataset_hash.get().trim().to_string(),
            case_count: case_count_value,
            passed: false,
            report: report_value,
        };
        spawn_local(async move {
            match api::evaluate_prompt_asset(&token_value, asset_id, payload).await {
                Ok(_) => {
                    notice.set(Some(
                        "人工评测记录已登记；该记录不能作为发布通过凭证".into(),
                    ));
                    reload();
                }
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    let run_live_evaluation = move |asset_id: i64, prompt_key: String| {
        let Some(token_value) = token.get() else {
            return;
        };
        if busy.get() {
            return;
        }
        busy.set(true);
        error.set(None);
        notice.set(None);
        spawn_local(async move {
            match api::run_prompt_asset_evaluation(&token_value, asset_id).await {
                Ok(job) => {
                    notice.set(Some(format!(
                        "{} 在线 Golden 评测已排队，作业 #{}",
                        prompt_key_label(&prompt_key),
                        job.id
                    )));
                    evaluation_job.set(Some(job));
                }
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    let refresh_evaluation_job = move |_| {
        let Some(token_value) = token.get() else {
            return;
        };
        let Some(job) = evaluation_job.get() else {
            return;
        };
        if busy.get() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            match api::get_prompt_evaluation_job(&token_value, job.id).await {
                Ok(updated) => {
                    if matches!(updated.status.as_str(), "succeeded" | "failed") {
                        notice.set(Some(match updated.status.as_str() {
                            "succeeded" => "在线 Golden 评测已完成".into(),
                            _ => updated
                                .error_message
                                .clone()
                                .unwrap_or_else(|| "在线 Golden 评测失败".into()),
                        }));
                        reload();
                    }
                    evaluation_job.set(Some(updated));
                }
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    let cancel_evaluation_job = move |_| {
        let Some(token_value) = token.get() else {
            return;
        };
        let Some(job) = evaluation_job.get() else {
            return;
        };
        if busy.get() || !matches!(job.status.as_str(), "pending" | "running") {
            return;
        }
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match api::cancel_prompt_evaluation_job(&token_value, job.id).await {
                Ok(updated) => {
                    notice.set(Some(if updated.status == "cancelled" {
                        "评测作业已取消".into()
                    } else {
                        "已请求取消；当前模型调用结束后将停止后续样本".into()
                    }));
                    evaluation_job.set(Some(updated));
                }
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    let poll_token = token;
    let poll_job = evaluation_job;
    let poll_busy = busy;
    let poll_error = error;
    let poll_notice = notice;
    let poll_assets = assets;
    let poll_events = events;
    let poll_callback = Closure::<dyn FnMut()>::new(move || {
        let Some(token_value) = poll_token.get() else {
            return;
        };
        let Some(job) = poll_job.get() else {
            return;
        };
        if poll_busy.get() || !matches!(job.status.as_str(), "pending" | "running") {
            return;
        }
        poll_busy.set(true);
        spawn_local(async move {
            match api::get_prompt_evaluation_job(&token_value, job.id).await {
                Ok(updated) => {
                    if matches!(
                        updated.status.as_str(),
                        "succeeded" | "failed" | "cancelled"
                    ) {
                        poll_notice.set(Some(match updated.status.as_str() {
                            "succeeded" => "在线 Golden 评测已完成".into(),
                            "cancelled" => "在线 Golden 评测已取消".into(),
                            _ => updated
                                .error_message
                                .clone()
                                .unwrap_or_else(|| "在线 Golden 评测失败".into()),
                        }));
                        if matches!(updated.status.as_str(), "succeeded" | "failed") {
                            match api::list_prompt_assets(&token_value).await {
                                Ok(value) => poll_assets.set(value),
                                Err(message) => poll_error.set(Some(message)),
                            }
                            match api::list_prompt_asset_events(&token_value).await {
                                Ok(value) => poll_events.set(value),
                                Err(message) => poll_error.set(Some(message)),
                            }
                        }
                    }
                    poll_job.set(Some(updated));
                }
                Err(message) => poll_error.set(Some(message)),
            }
            poll_busy.set(false);
        });
    });
    let poll_interval = web_sys::window().and_then(|window| {
        window
            .set_interval_with_callback_and_timeout_and_arguments_0(
                poll_callback.as_ref().unchecked_ref(),
                3_000,
            )
            .ok()
            .map(|interval_id| (window, interval_id))
    });
    if let Some((_, interval_id)) = poll_interval {
        PROMPT_EVALUATION_POLLERS.with(|pollers| {
            pollers.borrow_mut().insert(interval_id, poll_callback);
        });
        on_cleanup(move || {
            if let Some(window) = web_sys::window() {
                window.clear_interval_with_handle(interval_id);
            }
            PROMPT_EVALUATION_POLLERS.with(|pollers| {
                pollers.borrow_mut().remove(&interval_id);
            });
        });
    }

    view! {
        <main class="admin-page admin-prompt-registry-page">
            <header class="admin-topbar">
                <div class="admin-brand">
                    <div class="brand-mark"><img src="/images/fuxi.png" /></div>
                    <div><span class="eyebrow">"PROMPT GOVERNANCE"</span><strong>"伏羲智能决策建模工作台"</strong></div>
                </div>
                <nav class="admin-topbar-actions admin-page-nav" aria-label="页面导航">
                    <button class="button button-quiet" on:click=move |_| navigate_to_users("/admin", Default::default())>"用户管理"</button>
                    <button class="button button-quiet" on:click=move |_| navigate_to_intelligence("/admin/intelligence", Default::default())>"智能运营"</button>
                </nav>
            </header>
            <div class="admin-content">
                <section class="admin-heading">
                    <div><span class="yaml-kicker">"PROMPT REGISTRY"</span><h1>"提示词版本管理"</h1></div>
                    <button class="button button-secondary" on:click=move |_| reload() disabled=busy>"刷新"</button>
                </section>
                <div class="prompt-registry-summary" aria-label="提示词管理概览">
                    <div><span>"版本总数"</span><strong>{move || assets.get().len()}</strong></div>
                    <div><span>"当前生效"</span><strong>{move || assets.get().iter().filter(|asset| asset.status == "active").count()}</strong></div>
                    <div><span>"待评测草稿"</span><strong>{move || assets.get().iter().filter(|asset| asset.status == "draft").count()}</strong></div>
                    <div><span>"审计事件"</span><strong>{move || events.get().len()}</strong></div>
                </div>
                <Show when=move || error.get().is_some()>
                    <div class="error-msg" role="alert">{move || error.get().unwrap_or_default()}</div>
                </Show>
                <Show when=move || notice.get().is_some()>
                    <div class="prompt-success-msg" role="status">{move || notice.get().unwrap_or_default()}</div>
                </Show>
                <Show when=move || evaluation_job.get().is_some()>
                    <section class="prompt-evaluation-job" aria-live="polite">
                        <div class="admin-section-heading">
                            <div><span class="yaml-kicker">"LIVE GOLDEN JOB"</span><h2>"在线评测进度"</h2></div>
                            <button class="button button-secondary" disabled=busy on:click=refresh_evaluation_job>"刷新进度"</button>
                        </div>
                        <Show when=move || evaluation_job.get().is_some_and(|job| matches!(job.status.as_str(), "pending" | "running") && !job.cancel_requested)>
                            <button class="button button-quiet" disabled=busy on:click=cancel_evaluation_job>"取消评测"</button>
                        </Show>
                        <p>
                            "作业 #" {move || evaluation_job.get().map(|job| job.id).unwrap_or_default()}
                            {move || evaluation_job.get().and_then(|job| {
                                assets.get().into_iter().find(|asset| asset.id == job.prompt_asset_id)
                            }).map(|asset| format!(
                                " · {} {}",
                                prompt_key_label(&asset.prompt_key),
                                asset.version
                            )).unwrap_or_default()}
                            " · " {move || evaluation_job.get().map(|job| if job.cancel_requested { "取消中" } else { match job.status.as_str() {
                                "pending" => "排队中",
                                "running" => "运行中",
                                "succeeded" => "已完成",
                                "failed" => "失败",
                                "cancelled" => "已取消",
                                _ => "未知状态",
                            }}).unwrap_or("未知状态")}
                        </p>
                        <progress max=move || evaluation_job.get().map(|job| job.total_cases.max(1)).unwrap_or(1)
                            value=move || evaluation_job.get().map(|job| job.completed_cases).unwrap_or(0) />
                        <small>
                            {move || evaluation_job.get().map(|job| format!("{} / {} 条样本", job.completed_cases, job.total_cases)).unwrap_or_default()}
                        </small>
                        <Show when=move || evaluation_job.get().is_some_and(|job| job.status == "failed")>
                            <p class="error-msg" role="alert">{move || evaluation_job.get().and_then(|job| job.error_message).unwrap_or_else(|| "评测作业失败".into())}</p>
                        </Show>
                    </section>
                </Show>
                <div class="prompt-registry-forms">
                <section class="prompt-draft-panel prompt-registry-section">
                    <div class="admin-section-heading"><div><span class="yaml-kicker">"NEW VERSION"</span><h2>"创建提示词草稿"</h2></div></div>
                    <div class="admin-form-grid management-form-grid">
                        <WorkbenchSelect
                            label="提示词类别"
                            options=Signal::derive(prompt_key_options)
                            value=prompt_key
                            disabled=busy
                            placeholder=Signal::derive(|| "选择提示词类别".to_string())
                            on_change=Callback::new(move |value| prompt_key.set(value))
                        />
                        <label><span>"版本号"</span><input maxlength="64" prop:value=version on:input=move |event| version.set(event_target_value(&event)) /></label>
                        <label class="prompt-content-field"><span>"系统提示词内容"</span><textarea maxlength="100000" rows="9" prop:value=content on:input=move |event| content.set(event_target_value(&event))></textarea></label>
                    </div>
                    <button class="button button-primary" on:click=create_draft disabled=move || busy.get() || version.get().trim().is_empty() || content.get().trim().is_empty()>{move || if busy.get() { "处理中..." } else { "创建草稿" }}</button>
                </section>
                <section class="prompt-evaluation-panel prompt-registry-section">
                    <div class="admin-section-heading"><div><span class="yaml-kicker">"EVALUATION RECORD"</span><h2>"评测记录"</h2></div></div>
                    <div class="admin-form-grid management-form-grid">
                        <label><span>"数据集 SHA-256"</span><input maxlength="64" prop:value=dataset_hash on:input=move |event| dataset_hash.set(event_target_value(&event)) /></label>
                        <WorkbenchNumberInput
                            label="样本数"
                            value=case_count
                            min=1
                            step=1
                            disabled=busy
                        />
                        <label class="prompt-content-field"><span>"人工评测记录 JSON"</span><textarea rows="4" prop:value=report on:input=move |event| report.set(event_target_value(&event))></textarea></label>
                    </div>
                </section>
                </div>
                <section class="prompt-versions-section prompt-registry-section">
                    <div class="admin-section-heading"><div><span class="yaml-kicker">"VERSIONS"</span><h2>"版本与发布状态"</h2></div><span class="prompt-section-count">{move || assets.get().len()} " 个版本"</span></div>
                    <div class="prompt-assets-list">
                        <For each=move || assets.get() key=|asset| asset.id let:asset>
                            {
                                let label = prompt_key_label(&asset.prompt_key);
                                let status_label = prompt_status_label(&asset.status);
                                let status_class = match asset.status.as_str() {
                                    "active" => "prompt-status prompt-status-active",
                                    "evaluated" => "prompt-status prompt-status-evaluated",
                                    _ => "prompt-status",
                                };
                                let asset_id = asset.id;
                                let active_key = RwSignal::new(asset.prompt_key.clone());
                                let has_evaluation = asset.evaluation.is_some();
                                let evaluation_display = asset.evaluation.as_ref().map(|value| {
                                    serde_json::to_string_pretty(value).unwrap_or_default()
                                });
                                let can_evaluate = asset.status == "draft";
                                let can_run_live_evaluation = can_evaluate
                                    && matches!(asset.prompt_key.as_str(), "decision_planning" | "dsl_generation" | "optimization_proposal" | "decision_explanation" | "tester_judge");
                                let run_evaluation = run_live_evaluation;
                                let is_planning_prompt = asset.prompt_key == "decision_planning";
                                let is_optimization_prompt = asset.prompt_key == "optimization_proposal";
                                let is_explanation_prompt = asset.prompt_key == "decision_explanation";
                                let is_tester_judge_prompt = asset.prompt_key == "tester_judge";
                                let can_activate = asset.status == "evaluated" && asset.evaluation.as_ref().is_some_and(|value| value["passed"] == true);
                                let can_rollback = asset.status == "active";
                                let activate_token = token;
                                let rollback_token = token;
                                view! {
                                    <article class="prompt-asset-row">
                                        <div class="prompt-asset-heading">
                                            <strong>{label} " · " {asset.version.clone()}</strong>
                                            <span class=status_class>{status_label}</span>
                                        </div>
                                        <div class="prompt-asset-meta"><span>"SHA-256 " {asset.content_digest.clone()}</span><span>"创建者 " {asset.created_by.clone()}</span></div>
                                        <details class="prompt-asset-content-details"><summary>"查看提示词内容"</summary><pre class="prompt-content-preview">{asset.content.clone()}</pre></details>
                                        <Show when=move || has_evaluation>
                                            <details class="prompt-asset-evaluation-details"><summary>"查看评测报告"</summary><pre class="prompt-content-preview">{evaluation_display.clone().unwrap_or_default()}</pre></details>
                                        </Show>
                                        <div class="prompt-asset-actions">
                                            <Show when=move || can_evaluate>
                                                <button class="button button-secondary" disabled=busy on:click=move |_| record_evaluation(asset_id)>"登记人工记录"</button>
                                            </Show>
                                            <Show when=move || can_run_live_evaluation>
                                                <button class="button button-secondary" disabled=move || busy.get() || evaluation_job.get().is_some_and(|job| matches!(job.status.as_str(), "pending" | "running")) on:click=move |_| run_evaluation(
                                                    asset_id,
                                                    if is_planning_prompt { "decision_planning" } else if is_optimization_prompt { "optimization_proposal" } else if is_explanation_prompt { "decision_explanation" } else if is_tester_judge_prompt { "tester_judge" } else { "dsl_generation" }.into(),
                                                )>
                                                    {if is_planning_prompt {
                                                "运行规划 Golden（20 条样本）"
                                                    } else if is_optimization_prompt {
                                                "运行优化建议契约评测（20 条样本）"
                                                    } else if is_explanation_prompt {
                                                "运行解释轨迹契约评测（20 条样本）"
                                                    } else if is_tester_judge_prompt {
                                                "运行 Judge 合成契约评测（20 条样本）"
                                                    } else {
                                                "运行 DSL Golden（50 条样本）"
                                                    }}
                                                </button>
                                            </Show>
                                            <Show when=move || can_activate>
                                                <button class="button button-primary" disabled=busy on:click=move |_| {
                                                    let Some(token_value) = activate_token.get() else { return; };
                                                    busy.set(true);
                                                    error.set(None);
                                                    spawn_local(async move {
                                                        match api::activate_prompt_asset(&token_value, asset_id).await {
                                                            Ok(_) => { notice.set(Some("提示词版本已激活；重启服务后生效".into())); reload(); }
                                                            Err(message) => error.set(Some(message)),
                                                        }
                                                        busy.set(false);
                                                    });
                                                }>{"激活版本"}</button>
                                            </Show>
                                            <Show when=move || can_rollback>
                                                <button class="button button-quiet" disabled=busy on:click=move |_| {
                                                    let Some(token_value) = rollback_token.get() else { return; };
                                                    let key = active_key.get();
                                                    busy.set(true);
                                                    error.set(None);
                                                    spawn_local(async move {
                                                        match api::rollback_prompt_asset(&token_value, &key).await {
                                                            Ok(_) => { notice.set(Some("提示词版本已回滚；重启服务后生效".into())); reload(); }
                                                            Err(message) => error.set(Some(message)),
                                                        }
                                                        busy.set(false);
                                                    });
                                                }>{"回滚到上一版本"}</button>
                                            </Show>
                                        </div>
                                    </article>
                                }
                            }
                        </For>
                    </div>
                </section>
                <section class="prompt-audit-section prompt-registry-section">
                    <div class="admin-section-heading"><div><span class="yaml-kicker">"AUDIT TRAIL"</span><h2>"发布历史"</h2></div></div>
                    <div class="prompt-event-list">
                        <For each=move || events.get() key=|event| event.id let:event>
                            <div class="prompt-event-row"><strong>{prompt_key_label(&event.prompt_key)} " · " {event.version.clone()}</strong><span>{event.event_type.clone()} " · " {event.actor_username.clone()}</span><time>{event.created_at.format("%Y-%m-%d %H:%M:%S").to_string()}</time></div>
                        </For>
                    </div>
                </section>
            </div>
        </main>
    }
}
