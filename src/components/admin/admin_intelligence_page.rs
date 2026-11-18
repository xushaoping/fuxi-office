use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;

use crate::api;
use crate::components::auth::auth_token;
use crate::components::common::{SelectOption, WorkbenchSelect};
use crate::models::{
    AdminBusinessQualityMetricView, AgentOrchestrationHistoryItem,
    DecisionExplanationAdminMetricView, DecisionProject, ModelExperimentMetricView,
    ModelPerformanceMetricView, ModelUsageMetricView,
};

const ORCHESTRATION_PAGE_SIZE: u16 = 25;

fn orchestration_status_label(status: &str) -> &str {
    match status {
        "pending" => "等待中",
        "running" => "运行中",
        "succeeded" => "成功",
        "failed" => "失败",
        "cancelled" => "已取消",
        "skipped" => "已跳过",
        other => other,
    }
}

fn orchestration_usage_summary(usage: Option<&serde_json::Value>) -> String {
    let Some(usage) = usage else {
        return "Token 用量待记录".into();
    };
    let prompt = usage["prompt_tokens"].as_i64().unwrap_or(0);
    let completion = usage["completion_tokens"].as_i64().unwrap_or(0);
    let total = usage["total_tokens"]
        .as_i64()
        .unwrap_or(prompt + completion);
    if total == 0 {
        "Token 用量待记录".into()
    } else {
        format!("Token {total}（输入 {prompt} · 输出 {completion}）")
    }
}

#[component]
pub fn AdminIntelligencePage() -> impl IntoView {
    let token = auth_token();
    let navigate = use_navigate();
    let navigate_to_prompts = navigate.clone();
    let model_metrics = RwSignal::new(Vec::<ModelPerformanceMetricView>::new());
    let quality_metrics = RwSignal::new(Vec::<AdminBusinessQualityMetricView>::new());
    let experiment_metrics = RwSignal::new(Vec::<ModelExperimentMetricView>::new());
    let usage_metrics = RwSignal::new(Vec::<ModelUsageMetricView>::new());
    let explanation_metrics = RwSignal::new(Vec::<DecisionExplanationAdminMetricView>::new());
    let projects = RwSignal::new(Vec::<DecisionProject>::new());
    let selected_project_id = RwSignal::new(None::<i64>);
    let orchestration_history = RwSignal::new(Vec::<AgentOrchestrationHistoryItem>::new());
    let orchestration_loaded_for = RwSignal::new(None::<i64>);
    let orchestration_has_more = RwSignal::new(false);
    let orchestration_loading = RwSignal::new(false);
    let orchestration_error = RwSignal::new(None::<String>);
    let orchestration_request_generation = RwSignal::new(0_u64);
    let loading = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);

    let load_orchestration_history = move |project_id: i64, append: bool| {
        let Some(token_value) = token.get_untracked() else {
            return;
        };
        if append && orchestration_loading.get_untracked() {
            return;
        }
        let request_generation = orchestration_request_generation
            .get_untracked()
            .wrapping_add(1);
        orchestration_request_generation.set(request_generation);
        let before_id = if append {
            orchestration_history
                .get_untracked()
                .last()
                .map(|item| item.id)
        } else {
            None
        };
        if !append {
            orchestration_history.set(Vec::new());
            orchestration_has_more.set(false);
        }
        orchestration_loading.set(true);
        orchestration_error.set(None);
        spawn_local(async move {
            match api::list_project_orchestrations(
                &token_value,
                project_id,
                before_id,
                ORCHESTRATION_PAGE_SIZE,
            )
            .await
            {
                Ok(items) => {
                    if orchestration_request_generation.get_untracked() == request_generation
                        && selected_project_id.get_untracked() == Some(project_id)
                    {
                        orchestration_loaded_for.set(Some(project_id));
                        orchestration_has_more.set(items.len() == ORCHESTRATION_PAGE_SIZE as usize);
                        if append {
                            orchestration_history.update(|history| history.extend(items));
                        } else {
                            orchestration_history.set(items);
                        }
                    }
                }
                Err(message)
                    if orchestration_request_generation.get_untracked() == request_generation =>
                {
                    orchestration_error.set(Some(message));
                }
                Err(_) => {}
            }
            if orchestration_request_generation.get_untracked() == request_generation {
                orchestration_loading.set(false);
            }
        });
    };

    let reload = move || {
        let Some(token_value) = token.get() else {
            return;
        };
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            let mut errors = Vec::new();
            match api::list_model_performance_metrics(&token_value).await {
                Ok(items) => model_metrics.set(items),
                Err(message) => errors.push(message),
            }
            match api::list_business_quality_metrics(&token_value).await {
                Ok(items) => quality_metrics.set(items),
                Err(message) => errors.push(message),
            }
            match api::list_model_usage_metrics(&token_value).await {
                Ok(items) => usage_metrics.set(items),
                Err(message) => errors.push(message),
            }
            match api::list_model_experiment_metrics(&token_value).await {
                Ok(items) => experiment_metrics.set(items),
                Err(message) => errors.push(message),
            }
            match api::list_decision_explanation_admin_metrics(&token_value).await {
                Ok(items) => explanation_metrics.set(items),
                Err(message) => errors.push(message),
            }
            match api::list_projects(&token_value).await {
                Ok(items) => {
                    let current_id = selected_project_id.get_untracked();
                    if current_id.is_none_or(|id| !items.iter().any(|project| project.id == id)) {
                        let next_id = items.first().map(|project| project.id);
                        if current_id != next_id {
                            orchestration_request_generation.update(|generation| {
                                *generation = generation.wrapping_add(1);
                            });
                            orchestration_history.set(Vec::new());
                            orchestration_loaded_for.set(None);
                            orchestration_has_more.set(false);
                            orchestration_loading.set(false);
                            orchestration_error.set(None);
                            selected_project_id.set(next_id);
                        }
                    }
                    projects.set(items);
                }
                Err(message) => errors.push(message),
            }
            if !errors.is_empty() {
                error.set(Some(errors.join("；")));
            }
            loading.set(false);
        });
    };

    Effect::new(move |_| {
        if token.get().is_some() {
            reload();
        }
    });

    view! {
        <main class="admin-page admin-intelligence-page">
            <header class="admin-topbar">
                <div class="admin-brand">
                    <div class="brand-mark"><img src="/images/fuxi.png" /></div>
                    <div>
                        <span class="eyebrow">"DMN DECISION WORKSPACE"</span>
                        <strong>"伏羲智能决策建模工作台"</strong>
                    </div>
                </div>
                <nav class="admin-topbar-actions admin-page-nav admin-intelligence-nav" aria-label="管理导航">
                    <button class="button button-quiet" on:click=move |_| navigate("/admin", Default::default())>
                        <span aria-hidden="true">"←"</span> "用户管理"
                    </button>
                    <button class="button button-quiet" on:click=move |_| navigate_to_prompts("/admin/prompts", Default::default())>"提示词管理"</button>
                </nav>
            </header>
            <div class="admin-content">
                <section class="admin-heading">
                    <div>
                        <span class="yaml-kicker">"INTELLIGENCE OPERATIONS"</span>
                        <h1>"智能运营指标"</h1>
                        <p>"查看模型质量、业务反馈、资源消耗和实验分组表现。"</p>
                    </div>
                    <button class="button button-secondary" on:click=move |_| reload() disabled=loading>
                        {move || if loading.get() { "刷新中..." } else { "刷新指标" }}
                    </button>
                </section>
                <Show when=move || error.get().is_some()>
                    <div class="error-msg" role="alert">{move || error.get().unwrap_or_default()}</div>
                </Show>
                <div class="admin-intelligence-grid">
                    <section class="management-list-panel model-performance-panel">
                        <div class="admin-section-heading">
                            <div><span class="yaml-kicker">"MODEL PERFORMANCE"</span><h2>"模型性能指标"</h2></div>
                            <span class="admin-user-count">{move || model_metrics.get().len()} " 项模型/任务"</span>
                        </div>
                        <p class="admin-performance-note">"由智能体运行记录自动聚合；样本较少时仅供参考。"</p>
                        <Show when=move || !model_metrics.get().is_empty() fallback=|| view! { <div class="admin-empty">"暂无模型性能样本；完成分析、规划或 DSL 生成后会自动出现。"</div> }>
                            <div class="admin-model-metric-list">
                                <For each=move || model_metrics.get() key=|metric| format!("{}:{}", metric.model_name, metric.task_type) let:metric>
                                    <article class="admin-model-metric">
                                        {let task_label = match metric.task_type.as_str() {
                                            "analyze" => "需求分析".to_string(),
                                            "plan" => "决策规划".to_string(),
                                            "dsl" => "DSL 生成".to_string(),
                                            "change_intent" => "变更意图".to_string(),
                                            other => other.to_string(),
                                        };
                                        view! { <div><strong>{metric.model_name.clone()}</strong><span>{task_label}</span></div> }}
                                        <div class="admin-model-metric-values">
                                            <span>{format!("成功率 {:.1}%", metric.success_rate)}</span>
                                            <span>{format!("平均延迟 {} ms", metric.avg_latency_ms)}</span>
                                            <span>{format!("样本 {} · 成功 {}", metric.sample_count, metric.success_count)}</span>
                                            <span>{format!("更新于 {}", metric.last_updated.format("%Y-%m-%d %H:%M"))}</span>
                                        </div>
                                    </article>
                                </For>
                            </div>
                        </Show>
                    </section>
                    <section class="management-list-panel model-performance-panel">
                        <div class="admin-section-heading">
                            <div><span class="yaml-kicker">"BUSINESS QUALITY / 30D"</span><h2>"业务效果指标"</h2></div>
                            <span class="admin-user-count">{move || quality_metrics.get().len()} " 个项目"</span>
                        </div>
                        <p class="admin-performance-note">"统计已完成决策运行和人工回填；无法判断的样本不计入偏差率。"</p>
                        <Show when=move || !quality_metrics.get().is_empty() fallback=|| view! { <div class="admin-empty">"暂无业务运行反馈；发布版本并回填实际结果后会自动出现。"</div> }>
                            <div class="admin-model-metric-list">
                                <For each=move || quality_metrics.get() key=|metric| metric.project_id let:metric>
                                    <article class="admin-model-metric">
                                        <div><strong>{metric.project_name.clone()}</strong><span>{metric.owner_username.clone()}</span></div>
                                        <div class="admin-model-metric-values">
                                            <span>{format!("运行 {} · 成功 {}", metric.total_runs, metric.succeeded_runs)}</span>
                                            <span>{format!("反馈覆盖 {:.1}%", metric.feedback_coverage_percent)}</span>
                                            <span>{format!("明确反馈 {} · 偏差率 {:.1}%", metric.conclusive_outcome_runs, metric.mismatch_rate_percent)}</span>
                                            <span>{format!("最近运行 {}", metric.last_run_at.format("%Y-%m-%d %H:%M"))}</span>
                                        </div>
                                    </article>
                                </For>
                            </div>
                        </Show>
                    </section>
                    <section class="management-list-panel model-performance-panel">
                        <div class="admin-section-heading">
                            <div><span class="yaml-kicker">"MODEL USAGE / 30D"</span><h2>"Token 用量"</h2></div>
                            <span class="admin-user-count">{move || usage_metrics.get().len()} " 个阶段"</span>
                        </div>
                        <p class="admin-performance-note">"按阶段汇总请求、成功响应、token 和响应成本估算，并显示估算覆盖数。"</p>
                        <Show when=move || !usage_metrics.get().is_empty() fallback=|| view! { <div class="admin-empty">"暂无 token 用量记录。"</div> }>
                            <div class="admin-model-metric-list">
                                <For each=move || usage_metrics.get() key=|metric| metric.task_type.clone() let:metric>
                                    <article class="admin-model-metric">
                                        <div><strong>{metric.task_type.clone()}</strong><span>{format!("{} 次运行", metric.run_count)}</span></div>
                                        <div class="admin-model-metric-values">
                                            <span>{format!("传输尝试 {} 次 / 成功响应 {} 次", metric.transport_attempt_count, metric.request_count)}</span>
                                            <span>{format!("总 token {}", metric.total_tokens)}</span>
                                            <span>{match metric.estimated_cost_usd {
                                                Some(cost) => format!("估算成本 ${cost:.4}（{}/{} 次运行有估算）", metric.estimated_cost_run_count, metric.run_count),
                                                None => format!("无成本估算（0/{} 次运行）", metric.run_count),
                                            }}</span>
                                            <span>{format!("输入 {} · 输出 {}", metric.prompt_tokens, metric.completion_tokens)}</span>
                                            <span>{format!("更新于 {}", metric.last_updated.format("%Y-%m-%d %H:%M"))}</span>
                                        </div>
                                    </article>
                                </For>
                            </div>
                        </Show>
                    </section>
                    <section class="management-list-panel model-performance-panel">
                        <div class="admin-section-heading">
                            <div><span class="yaml-kicker">"MODEL EXPERIMENTS"</span><h2>"A/B 实验"</h2></div>
                            <span class="admin-user-count">{move || experiment_metrics.get().len()} " 个分组"</span>
                        </div>
                        <p class="admin-performance-note">"按项目稳定分组；样本量达到 30 前不作显著性结论。"</p>
                        <Show when=move || !experiment_metrics.get().is_empty() fallback=|| view! { <div class="admin-empty">"暂无已启用的模型实验。"</div> }>
                            <div class="admin-model-metric-list">
                                <For each=move || experiment_metrics.get() key=|metric| format!("{}:{}:{}", metric.task_type, metric.experiment_name, metric.variant) let:metric>
                                    <article class="admin-model-metric">
                                        {let task_label = match metric.task_type.as_str() {
                                            "requirement_analysis" => "需求分析",
                                            "decision_planning" => "决策规划",
                                            "dsl_generation" => "DSL 生成",
                                            "change_intent" => "变更意图",
                                            other => other,
                                        };
                                        view! { <div><strong>{metric.experiment_name.clone()}</strong><span>{format!("{} / {}", task_label, metric.variant)}</span></div> }}
                                        <div class="admin-model-metric-values">
                                            <span>{format!("成功率 {:.1}%", metric.success_rate)}</span>
                                            <span>{format!("95% 区间 {:.1}% - {:.1}%", metric.success_rate_ci_lower, metric.success_rate_ci_upper)}</span>
                                            <span>{match (metric.treatment_minus_control_ci_lower, metric.treatment_minus_control_ci_upper) {
                                                (Some(lower), Some(upper)) => format!("处理效应 95% 区间 {:+.1} 至 {:+.1} 个百分点", lower, upper),
                                                _ => "处理效应区间待两组均有样本".to_string(),
                                            }}</span>
                                            <span>{format!("样本 {} · 成功 {}", metric.sample_count, metric.success_count)}</span>
                                            <span>{if metric.treatment_effect_significant { "差值区间未跨零" } else if metric.comparison_sample_size_reached { "差值区间跨零，暂未显示显著差异" } else { "至少一组样本量不足，暂不判断" }}</span>
                                            <span>{match metric.treatment_percent {
                                                Some(percent) if metric.treatment_percent_config_count <= 1 => format!("处理组配置比例 {}%", percent),
                                                Some(_) => format!("警告：检测到 {} 种处理组比例配置", metric.treatment_percent_config_count),
                                                None => "旧运行记录无分流比例审计数据".to_string(),
                                            }}</span>
                                            <span>{match metric.assignment_integrity_percent {
                                                Some(percent) => format!("哈希桶一致率 {:.1}%（审计样本 {}）", percent, metric.audit_sample_count),
                                                None => "哈希桶审计数据待累积".to_string(),
                                            }}</span>
                                        </div>
                                    </article>
                                </For>
                            </div>
                        </Show>
                    </section>
                    <section class="management-list-panel model-performance-panel">
                        <div class="admin-section-heading">
                            <div><span class="yaml-kicker">"DECISION EXPLANATIONS / 30D"</span><h2>"决策解释生成与可读性"</h2></div>
                            <span class="admin-user-count">{move || explanation_metrics.get().len()} " 个模型/版本组"</span>
                        </div>
                        <p class="admin-performance-note">"生成量按创建时间统计；P50/P95 仅基于成功且完成的延迟样本。审阅标签是事件数，样本不足时仅展示观测值，不代表质量达标。"</p>
                        <Show when=move || !explanation_metrics.get().is_empty() fallback=|| view! { <div class="admin-empty">"最近 30 天暂无决策解释生成记录。"</div> }>
                            <div class="admin-model-metric-list">
                                <For each=move || explanation_metrics.get() key=|metric| format!("{:?}:{}", metric.model, metric.prompt_version) let:metric>
                                    <article class="admin-model-metric">
                                        <div>
                                            <strong>{metric.model.clone().unwrap_or_else(|| "模型未记录".to_string())}</strong>
                                            <span>{format!("提示词版本 {}", metric.prompt_version)}</span>
                                        </div>
                                        <div class="admin-model-metric-values">
                                            <span>{format!("生成 {} · 成功 {} · 失败 {} · 运行中 {}", metric.total_count, metric.succeeded_count, metric.failed_count, metric.running_count)}</span>
                                            <span>{match (metric.p50_latency_ms, metric.p95_latency_ms) {
                                                (Some(p50), Some(p95)) => format!("生成延迟 P50 {p50} ms · P95 {p95} ms（{}/{} 次成功完成）", metric.latency_sample_count, metric.succeeded_count),
                                                _ => format!("生成延迟暂无分位数（有效样本 {}/{} 次成功完成）", metric.latency_sample_count, metric.succeeded_count),
                                            }}</span>
                                            <span>{format!("审阅解释 {} 个 · 独立首审 {} 次", metric.reviewed_explanation_count, metric.review_count)}</span>
                                            <span>{format!("可读 {} · 需修改 {} · 无法判断 {}（分母 {} 次独立首审）", metric.readable_count, metric.needs_revision_count, metric.inconclusive_count, metric.review_count)}</span>
                                        </div>
                                    </article>
                                </For>
                            </div>
                        </Show>
                    </section>
                    <section class="management-list-panel model-performance-panel admin-orchestration-history">
                        <div class="admin-section-heading">
                            <div><span class="yaml-kicker">"AGENT ORCHESTRATIONS"</span><h2>"Agent 编排历史"</h2></div>
                            <span class="admin-user-count">{move || orchestration_history.get().len()} " 条已加载"</span>
                        </div>
                        <div class="admin-orchestration-controls">
                            <WorkbenchSelect
                                id="orchestration-project-select"
                                class="orchestration-project-select"
                                label="项目"
                                placeholder=Signal::derive(|| "选择项目".to_string())
                                options=Signal::derive(move || {
                                    projects
                                        .get()
                                        .into_iter()
                                        .map(|project| SelectOption::new(project.id.to_string(), project.name))
                                        .collect()
                                })
                                value=Signal::derive(move || {
                                    selected_project_id
                                        .get()
                                        .map(|id| id.to_string())
                                        .unwrap_or_default()
                                })
                                disabled=Signal::derive(move || {
                                    projects.get().is_empty() || orchestration_loading.get()
                                })
                                on_change=Callback::new(move |value: String| {
                                    orchestration_request_generation.update(|generation| {
                                        *generation = generation.wrapping_add(1);
                                    });
                                    orchestration_history.set(Vec::new());
                                    orchestration_loaded_for.set(None);
                                    orchestration_has_more.set(false);
                                    orchestration_loading.set(false);
                                    orchestration_error.set(None);
                                    selected_project_id.set(value.parse::<i64>().ok());
                                })
                            />
                            <button
                                class="button button-quiet"
                                on:click=move |_| {
                                    if let Some(project_id) = selected_project_id.get() {
                                        load_orchestration_history(project_id, false);
                                    }
                                }
                                disabled=move || selected_project_id.get().is_none() || orchestration_loading.get()
                                title="加载或刷新编排历史"
                            >
                                {move || {
                                    if orchestration_loading.get() {
                                        "读取中..."
                                    } else if orchestration_loaded_for.get() == selected_project_id.get() {
                                        "刷新"
                                    } else {
                                        "加载历史"
                                    }
                                }}
                            </button>
                        </div>
                        <Show when=move || orchestration_error.get().is_some()>
                            <div class="error-msg" role="alert">{move || orchestration_error.get().unwrap_or_default()}</div>
                        </Show>
                        <Show
                            when=move || !orchestration_history.get().is_empty()
                            fallback=move || view! {
                                <div class="admin-empty">
                                    {move || {
                                        if orchestration_loading.get() {
                                            "读取编排历史...".to_string()
                                        } else if orchestration_loaded_for.get() == selected_project_id.get() {
                                            "该项目暂无 Agent 编排记录。".to_string()
                                        } else {
                                            "点击“加载历史”查看该项目的 Agent 编排记录。".to_string()
                                        }
                                    }}
                                </div>
                            }
                        >
                            <div class="admin-orchestration-list">
                                <For each=move || orchestration_history.get() key=|item| item.id let:item>
                                    {let heading = format!("编排 #{} · {}", item.id, item.current_stage);
                                    let metadata = format!(
                                        "{} · {} · {}",
                                        orchestration_status_label(&item.status),
                                        item.trigger_kind,
                                        item.created_at.format("%Y-%m-%d %H:%M:%S")
                                    );
                                    let step_count = item.steps.len();
                                    let steps = RwSignal::new(item.steps.clone());
                                    let has_steps = step_count > 0;
                                    view! {
                                    <article class="admin-orchestration-item">
                                        <header>
                                            <div>
                                                <strong>{heading}</strong>
                                                <span>{metadata}</span>
                                            </div>
                                            <span>{format!("{} 步", step_count)}</span>
                                        </header>
                                        <Show when=move || has_steps fallback=|| view! { <p class="admin-performance-note">"尚无步骤记录"</p> }>
                                            <div class="admin-orchestration-steps">
                                                <For each=move || steps.get() key=|step| step.id let:step>
                                                    <div class="admin-orchestration-step">
                                                        <div>
                                                            <strong>{format!("{}. {} / {}", step.step_no, step.agent_role, step.task_kind)}</strong>
                                                            <span>{format!("{}{}", orchestration_status_label(&step.status), step.model.as_ref().map(|model| format!(" · {model}")).unwrap_or_default())}</span>
                                                        </div>
                                                        <div class="admin-orchestration-step-meta">
                                                            <span>{orchestration_usage_summary(step.usage.as_ref())}</span>
                                                            <span>{format!("提示词 {} · 产物 {} v{}", step.prompt_version.as_deref().unwrap_or("未知"), step.artifact_kind.as_deref().unwrap_or("无"), step.artifact_schema_version)}</span>
                                                            {step.artifact_digest.as_ref().map(|digest| view! { <span>{format!("摘要 {}", digest.chars().take(12).collect::<String>())}</span> })}
                                                            {step.error.as_ref().and_then(|error| error.get("message")).and_then(serde_json::Value::as_str).map(|message| view! { <span class="admin-orchestration-error">{message.to_string()}</span> })}
                                                        </div>
                                                    </div>
                                                </For>
                                            </div>
                                        </Show>
                                    </article>
                                    }}
                                </For>
                            </div>
                        </Show>
                        <Show when=move || orchestration_has_more.get()>
                            <div class="admin-orchestration-more">
                                <button
                                    class="button button-secondary"
                                    on:click=move |_| {
                                        if let Some(project_id) = selected_project_id.get() {
                                            load_orchestration_history(project_id, true);
                                        }
                                    }
                                    disabled=move || orchestration_loading.get()
                                >
                                    {move || if orchestration_loading.get() { "读取中..." } else { "加载更早记录" }}
                                </button>
                            </div>
                        </Show>
                    </section>
                </div>
            </div>
        </main>
    }
}
