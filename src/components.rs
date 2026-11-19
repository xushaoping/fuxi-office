mod dmn_app;
mod yaml_artifacts;
mod yaml_structure;

pub mod admin;
pub mod analytics;
pub mod auth;
pub mod common;
pub mod knowledge;
pub mod model;
pub mod projects;
pub mod workspace;

use std::collections::{BTreeMap, HashSet};

use gloo_storage::{LocalStorage, Storage};
use leptos::ev::KeyboardEvent;
use leptos::leptos_dom::helpers::{request_animation_frame, window_event_listener};
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
use crate::components::common::{
    SelectOption, WorkbenchCheckbox, WorkbenchNumberInput, WorkbenchSelect,
};
use crate::models::{
    AdminUser, AuditEvent, BehaviorComparisonReport, CreateDecisionTestCase, CreateDecisionVersion,
    CreateUser, DecisionAnalyticsReport, DecisionDriftAutomationRule, DecisionDriftEvent,
    DecisionDriftImpactReport, DecisionDriftScanWindow, DecisionOptimizationProposal,
    DecisionProject, DecisionServiceApiKey, DecisionServiceRun, DecisionTestCase, DecisionVersion,
    DeviationAnalysis, ExecutionTrace, KnowledgeCitation, KnowledgeSource, ModelChangeIntent,
    ModelLibraryItem, OptimizationAutodraftShadowEvaluation, OptimizationSimulationReport,
    PatternLibraryAnalytics, PatternLineageItem, PatternRecommendation,
    PatternRecommendationFeedback, ProjectMember, PublishedDecisionService, RegressionTestReport,
    SaveDecisionRunOutcome, SaveDecisionTemplate, SaveKnowledgeSource,
    UpdateDecisionDriftAutomation, UpdateDecisionDriftEventReview,
    UpdateDecisionDriftScanWindowReview, UpdateDeviationAnalysisReview, UpdateUser,
    VersionChangeReview, VersionDiffEntry, VersionVerificationReport,
};
use crate::state::AppState;
use crate::state::WorkspaceDraftSnapshot;
use crate::utils::use_keyboard_shortcut;
use crate::workflow::{
    Clarification, ClarificationAnswer, DecisionBrief, DecisionField, DecisionPlan,
    DecisionWorkflowState, MAX_MODEL_CHANGE_REPAIRS, PendingRequirementReview,
    append_orchestration_progress, is_repairable_model_change_error,
    optimization_proposal_handoff_prompt,
};
use web_sys::AbortController;

const DRIFT_AUTOMATION_OPTIONS: &[(&str, &str)] = &[
    ("FREQUENT_FALLBACK", "频繁回退"),
    ("HIGH_FAILURE_RATE", "失败率偏高"),
    ("HIGH_MISMATCH_RATE", "业务偏差率偏高"),
    ("HIGH_P95_LATENCY", "P95 延迟偏高"),
    ("INPUT_DISTRIBUTION_DRIFT", "输入分布漂移"),
    ("INPUT_FIELD_MISSINGNESS_DRIFT", "输入字段缺失率变化"),
    ("LOW_FEEDBACK_COVERAGE", "反馈覆盖不足"),
    ("LOW_RULE_COVERAGE", "规则覆盖偏低"),
    ("NO_RECENT_RUNS", "近期无运行"),
    ("RULE_FEEDBACK_MISMATCH", "规则反馈不一致"),
    ("RULE_HIT_DRIFT", "规则命中漂移"),
];

fn simple_direct_workflow_eligible(workflow: &DecisionWorkflowState) -> bool {
    workflow.simple_direct_eligible
}

fn newest_drift_scan_windows(
    mut windows: Vec<DecisionDriftScanWindow>,
) -> Vec<DecisionDriftScanWindow> {
    windows.reverse();
    windows
}

// Import from modular components
use admin::{AdminIntelligencePage, AdminPromptRegistryPage, AdminUsersPage};
use auth::{AuthExpiryWatcher, LoginPage, RequireAuth, TOKEN_KEY, auth_token};
use dmn_app::business_tests::{BusinessTestCaseSummary, TestCaseFilter};
use dmn_app::context::provide_app_context;
use dmn_app::services::{
    ServiceContractList, ServiceCredentialsPanel, ServiceInvocationForm, ServiceRunHistory,
};
use knowledge::KnowledgeLibraryPanel;
use projects::ProjectManagementPage;
use workspace::{
    CopyStatus, GenerationMode, MobileModelFocus, ResourceView, RunStatus, WorkspaceView,
    YAML_GRAPH_NODE_HEIGHT, YAML_GRAPH_NODE_WIDTH, YAML_GRAPH_WIDTH, YamlGraphEdge, YamlGraphNode,
    agent_activity_for_step, apply_result, apply_result_message, copy_text_to_clipboard,
    escape_html, highlight_code, highlight_json, highlight_value, highlight_xml,
    highlight_xml_text, highlight_yaml_line, incremental_result_message, indexed,
    optimization_proposal_actions, optimization_proposal_status_label, public_service_curl,
    public_service_endpoint, service_input_template, service_sample_value, summary_names,
    timeline_item_class, timeline_item_text, token_span, workflow_status_label, yaml_get,
    yaml_graph_edge_anchors, yaml_graph_edge_path, yaml_graph_height, yaml_graph_knowledge_points,
    yaml_graph_label, yaml_joined_list, yaml_list, yaml_map, yaml_text,
};

// Re-export the new modular DmnApp components
pub use dmn_app::execution::ExecutionTraceView;
pub use dmn_app::layout::{Header, ProjectBar, WorkspaceNavigation, WorkspaceOverview};
pub use dmn_app::team::{AuditLogPanel, MemberManagementPanel, RollbackPanel};
pub use dmn_app::types::ChatMessage;
pub use dmn_app::version::{
    SavedVersionReviewView, VerificationReportView, VersionChangeReviewView, VersionChangeRow,
};
use dmn_app::version::{VersionGovernancePanel, VersionHistoryPanel};
pub(crate) use yaml_artifacts::{ArtifactTabs, yaml_summary};

pub use dmn_app::workflow::{
    ClarificationInput, DecisionBriefView, DecisionPlanView, RequirementAnalysisProgress,
    TesterReportView, WorkflowFieldRow,
};

// 已提取的组件定义已移至 dmn_app 模块
// ClarificationInput, WorkflowFieldRow, DecisionBriefView, RequirementAnalysisProgress, DecisionPlanView
// VersionChangeReviewView, SavedVersionReviewView, VersionChangeRow, VerificationReportView

fn timeline_entries(steps: Vec<String>) -> Vec<(usize, String)> {
    steps.into_iter().enumerate().collect()
}

fn scroll_to_workflow_anchor(id: &'static str) {
    request_animation_frame(move || {
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        let Some(element) = document.get_element_by_id(id) else {
            return;
        };
        element.scroll_into_view_with_bool(false);
    });
}

fn load_compatible_workspace_draft(
    app_state: &AppState,
    project_id: i64,
    base_version_id: Option<i64>,
) -> Option<WorkspaceDraftSnapshot> {
    let snapshot = app_state.load_workspace_draft(project_id)?;
    if snapshot.project_id != project_id {
        app_state.clear_workspace_draft(project_id);
        return None;
    }
    if snapshot.base_version_id != base_version_id {
        let mut safe_snapshot = snapshot;
        safe_snapshot.base_version_id = base_version_id;
        safe_snapshot.pending_review = None;
        safe_snapshot.pending_candidate_id = None;
        safe_snapshot.pending_knowledge_citations.clear();
        safe_snapshot.review_confirmed = false;
        safe_snapshot.dsl = None;
        safe_snapshot.source_dsl.clear();
        safe_snapshot.xml = None;
        if safe_snapshot.modify_mode {
            safe_snapshot.prompt.clear();
            safe_snapshot.modify_mode = false;
        }
        safe_snapshot.workspace_view = "discovery".into();
        safe_snapshot.version_mismatch = true;
        return Some(safe_snapshot);
    }
    Some(snapshot)
}

fn workspace_view_key(view: WorkspaceView) -> &'static str {
    match view {
        WorkspaceView::Discovery => "discovery",
        WorkspaceView::Model => "model",
        WorkspaceView::Validation => "validation",
        WorkspaceView::Services => "services",
        WorkspaceView::Analytics => "analytics",
        WorkspaceView::Versions => "versions",
        WorkspaceView::Collaboration => "collaboration",
        WorkspaceView::Resources => "resources",
    }
}

fn workspace_view_from_key(value: &str) -> WorkspaceView {
    match value {
        "model" => WorkspaceView::Model,
        "validation" => WorkspaceView::Validation,
        "services" => WorkspaceView::Services,
        "analytics" => WorkspaceView::Analytics,
        "versions" => WorkspaceView::Versions,
        "collaboration" => WorkspaceView::Collaboration,
        "resources" => WorkspaceView::Resources,
        _ => WorkspaceView::Discovery,
    }
}

fn workspace_view_storage_key(project_id: i64) -> String {
    format!("fuxi-dmn:workspace-view:{project_id}")
}

fn load_workspace_view(project_id: i64) -> WorkspaceView {
    LocalStorage::get::<String>(&workspace_view_storage_key(project_id))
        .ok()
        .map(|key| workspace_view_from_key(&key))
        .unwrap_or(WorkspaceView::Discovery)
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
    // A single application state owner survives route changes and is available
    // to every feature view (notifications, theme and responsive UI state).
    let app_state = AppState::new();
    let theme = app_state.ui.theme;
    provide_context(app_state);

    Effect::new(move |_| {
        if let Some(root) = document().document_element() {
            let classes = root
                .class_name()
                .split_whitespace()
                .filter(|class| !matches!(*class, "theme-light" | "theme-dark"))
                .map(str::to_owned)
                .collect::<Vec<_>>()
                .join(" ");
            let _ = root.set_attribute("class", &format!("{} {}", classes, theme.get().class()));
        }
    });

    view! {
        <common::ToastContainer />
        <Router>
            <AuthExpiryWatcher />
            <Routes fallback=|| view! { <p>"加载中..."</p> }>
                <Route path=path!("/") view=move || view! { <RequireAuth><DmnApp /></RequireAuth> } />
                <Route path=path!("/admin") view=move || view! { <RequireAuth><AdminUsersPage /></RequireAuth> } />
                <Route path=path!("/admin/intelligence") view=move || view! { <RequireAuth><AdminIntelligencePage /></RequireAuth> } />
                <Route path=path!("/admin/prompts") view=move || view! { <RequireAuth><AdminPromptRegistryPage /></RequireAuth> } />
                <Route path=path!("/projects") view=move || view! { <RequireAuth><ProjectManagementPage /></RequireAuth> } />
                <Route path=path!("/login") view=LoginPage />
            </Routes>
        </Router>
    }
}

fn audit_event_kind(event_type: &str) -> &'static str {
    match event_type {
        "test_cases.generated"
        | "test_case.approved"
        | "test_case.approval_revoked"
        | "tester_proposal.converted" => "test-case",
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
        "tester_proposal.converted" => "提案转用例",
        "decision_service.invoked" => "服务调用",
        "project.member_added" => "成员添加",
        "project.member_removed" => "成员移除",
        _ => "其他事件",
    }
}

fn orchestration_event_update(event: &SseEvent) -> Option<(String, bool)> {
    let role_label = |role: &str| match role {
        "analyst" => "分析师",
        "facilitator" => "澄清主持人",
        "architect" => "架构师",
        "validator" => "校验器",
        "engineer" => "工程师",
        "tester" => "测试员",
        _ => "智能体",
    };
    let artifact_label = |kind: &str| match kind {
        "decision_brief" => "需求卡",
        "clarification_context" | "clarification_state" => "澄清上下文",
        "confirmed_decision_plan" | "decision_plan" => "决策方案",
        "tester_report" => "测试提案",
        "compiled_dmn_candidate" => "已校验模型候选",
        _ => "结构化产物",
    };
    match event {
        SseEvent::Stage(payload) => {
            let name = payload["name"].as_str()?;
            let label = match name {
                "analysis" => "需求分析",
                "facilitator" => "多轮澄清",
                "architect" => "方案规划",
                "engineer" => "DSL 工程",
                _ => return None,
            };
            Some((format!("编排阶段：{label}"), false))
        }
        SseEvent::Agent(payload) => {
            let role = role_label(payload["role"].as_str()?);
            let status = match payload["status"].as_str()? {
                "running" => "正在运行",
                "completed" => "已完成",
                "failed" => "失败",
                _ => "状态更新",
            };
            Some((format!("{role}{status}"), true))
        }
        SseEvent::Artifact(payload) => Some((
            format!("已生成{}", artifact_label(payload["kind"].as_str()?)),
            true,
        )),
        SseEvent::Handoff(payload) => Some((
            format!(
                "{} → {}：已交接{}",
                role_label(payload["fromRole"].as_str()?),
                role_label(payload["toRole"].as_str()?),
                artifact_label(payload["artifactKind"].as_str()?),
            ),
            true,
        )),
        _ => None,
    }
}

#[component]
pub fn DmnApp() -> impl IntoView {
    let token = auth_token();
    let (user_id, set_user_id) = signal(String::new());
    let (user_role, set_user_role) = signal(String::new());
    let (projects, set_projects) = signal(Vec::<DecisionProject>::new());
    let (current_project, set_current_project) = signal(Option::<DecisionProject>::None);
    let (current_version, set_current_version) = signal(Option::<DecisionVersion>::None);
    let (model_library, set_model_library) = signal(Vec::<ModelLibraryItem>::new());
    let (resource_view, set_resource_view) = signal(ResourceView::Knowledge);
    let (pending_knowledge_citations, set_pending_knowledge_citations) =
        signal(Vec::<KnowledgeCitation>::new());
    let (model_library_filter, set_model_library_filter) = signal("all".to_string());
    let (model_library_query, set_model_library_query) = signal(String::new());
    let (pattern_recommendations, set_pattern_recommendations) =
        signal(Vec::<PatternRecommendation>::new());
    let (pattern_lineage, set_pattern_lineage) = signal(Vec::<PatternLineageItem>::new());
    let (pattern_lineage_busy, set_pattern_lineage_busy) = signal(false);
    let (pattern_analytics, set_pattern_analytics) =
        signal(Option::<PatternLibraryAnalytics>::None);
    let (pattern_recommendations_busy, set_pattern_recommendations_busy) = signal(false);
    let (pattern_feedback_busy, set_pattern_feedback_busy) = signal(false);
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
    let (behavior_comparison, set_behavior_comparison) =
        signal(Option::<BehaviorComparisonReport>::None);
    let (regression_report, set_regression_report) = signal(Option::<RegressionTestReport>::None);
    let (regression_loading, set_regression_loading) = signal(false);
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
    let (acceptance_tab, set_acceptance_tab) = signal("cases".to_string());
    let (decision_test_cases, set_decision_test_cases) = signal(Vec::<DecisionTestCase>::new());
    let (selected_test_case_ids, set_selected_test_case_ids) = signal(HashSet::<i64>::new());
    let (test_case_name, set_test_case_name) = signal(String::new());
    let (test_case_input, set_test_case_input) = signal("{}".to_string());
    let (test_case_expected, set_test_case_expected) = signal("{}".to_string());
    let (test_case_busy, set_test_case_busy) = signal(false);
    let (test_case_running, set_test_case_running) = signal(false);
    let (test_case_run_summary, set_test_case_run_summary) = signal(Option::<String>::None);
    let (test_case_filter, set_test_case_filter) = signal(TestCaseFilter::All);
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
    let (analytics_thresholds, set_analytics_thresholds) =
        signal(crate::models::DecisionAnalyticsThresholds::default());
    let (analytics_thresholds_loading, set_analytics_thresholds_loading) = signal(false);
    let (analytics_thresholds_busy, set_analytics_thresholds_busy) = signal(false);
    let (analytics_thresholds_error, set_analytics_thresholds_error) =
        signal(Option::<String>::None);
    let (optimization_proposals, set_optimization_proposals) =
        signal(Vec::<DecisionOptimizationProposal>::new());
    let (drift_events, set_drift_events) = signal(Vec::<DecisionDriftEvent>::new());
    let (drift_scan_windows, set_drift_scan_windows) =
        signal(Vec::<DecisionDriftScanWindow>::new());
    let (deviation_analyses, set_deviation_analyses) = signal(Vec::<DeviationAnalysis>::new());
    let (drift_impacts, set_drift_impacts) =
        signal(BTreeMap::<i64, DecisionDriftImpactReport>::new());
    let (drift_rule, set_drift_rule) = signal(Option::<DecisionDriftAutomationRule>::None);
    let (drift_rule_enabled, set_drift_rule_enabled) = signal(false);
    let (drift_rule_codes, set_drift_rule_codes) = signal(Vec::<String>::new());
    let drift_rule_cooldown = RwSignal::new("24".to_string());
    let (drift_webhook_url, set_drift_webhook_url) = signal(String::new());
    let (drift_rule_busy, set_drift_rule_busy) = signal(false);
    let (drift_rule_error, set_drift_rule_error) = signal(Option::<String>::None);
    let (drift_event_busy, set_drift_event_busy) = signal(false);
    let (drift_impact_busy, set_drift_impact_busy) = signal(None::<i64>);
    let (drift_event_error, set_drift_event_error) = signal(Option::<String>::None);
    let (proposal_busy, set_proposal_busy) = signal(false);
    let (proposal_error, set_proposal_error) = signal(Option::<String>::None);
    let (simulation_candidate_version_id, set_simulation_candidate_version_id) =
        signal(None::<i64>);
    let (optimization_simulation, set_optimization_simulation) =
        signal(Option::<OptimizationSimulationReport>::None);
    let (simulation_busy, set_simulation_busy) = signal(false);
    let (shadow_evaluations, set_shadow_evaluations) =
        signal(Vec::<OptimizationAutodraftShadowEvaluation>::new());
    let (shadow_cursor, set_shadow_cursor) = signal(0_i64);
    let (shadow_has_more, set_shadow_has_more) = signal(false);
    let (shadow_busy, set_shadow_busy) = signal(false);
    let (shadow_error, set_shadow_error) = signal(Option::<String>::None);
    let (shadow_request_generation, set_shadow_request_generation) = signal(0_u64);
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
    let (workflow_facilitating, set_workflow_facilitating) = signal(false);
    let (workflow_controller, set_workflow_controller) = signal(Option::<AbortController>::None);
    let (workflow_orchestration_id, set_workflow_orchestration_id) = signal(None::<i64>);
    let (workflow_orchestration_stage, set_workflow_orchestration_stage) = signal(0usize);
    let (workflow_orchestration_events, set_workflow_orchestration_events) =
        signal(Vec::<String>::new());
    let (workflow_planning, set_workflow_planning) = signal(false);
    let (workflow_confirming, set_workflow_confirming) = signal(false);
    let (workflow_error, set_workflow_error) = signal(Option::<String>::None);
    let (pending_requirement_review, set_pending_requirement_review) =
        signal(Option::<PendingRequirementReview>::None);
    let (selected_review_model, set_selected_review_model) = signal(Option::<String>::None);
    let (review_busy, set_review_busy) = signal(false);
    let (engineer_retry_id, set_engineer_retry_id) = signal(Option::<i64>::None);
    let (workflow_retry_id, set_workflow_retry_id) = signal(Option::<i64>::None);
    let (plan_retry_id, set_plan_retry_id) = signal(Option::<i64>::None);
    let (facilitator_retry_id, set_facilitator_retry_id) = signal(Option::<i64>::None);
    let (clarification_answers, set_clarification_answers) =
        signal(BTreeMap::<String, String>::new());
    let (prompt, set_prompt) = signal(String::new());
    let (requirement_importing, set_requirement_importing) = signal(false);
    let (requirement_import_error, set_requirement_import_error) = signal(None::<String>);
    let (messages, set_messages) = signal(Vec::<ChatMessage>::new());
    let (steps, set_steps) = signal(Vec::<String>::new());
    let (assistant_delta, set_assistant_delta) = signal(String::new());
    let (agent_activity, set_agent_activity) = signal("等待新的建模任务".to_string());
    let (dsl, set_dsl) = signal(Option::<String>::None);
    let (source_dsl, set_source_dsl) = signal(String::new());
    let (source_dsl_checkpoint, set_source_dsl_checkpoint) = signal(String::new());
    let (xml, set_xml) = signal(Option::<String>::None);
    let (error, set_error) = signal(Option::<String>::None);
    let (status, set_status) = signal(RunStatus::Idle);
    let (generation_mode, set_generation_mode) = signal(GenerationMode::Create);
    let (controller, set_controller) = signal(Option::<AbortController>::None);
    let (active_orchestration_id, set_active_orchestration_id) = signal(None::<i64>);
    let (workspace_view, set_workspace_view) = signal(WorkspaceView::Discovery);
    let (prompt_checkpoint, set_prompt_checkpoint) = signal(String::new());
    let (draft_to_restore, set_draft_to_restore) = signal(Option::<WorkspaceDraftSnapshot>::None);
    let workflow_current = Signal::derive(move || {
        decision_workflow.get().as_ref().is_some_and(|workflow| {
            workflow.matches_requirement(
                &prompt.get(),
                current_version.get().map(|version| version.id),
            )
        })
    });
    let workflow_answers_changed = Signal::derive(move || {
        decision_workflow.get().as_ref().is_some_and(|workflow| {
            workflow.plan.is_some()
                && workflow
                    .answers
                    .iter()
                    .map(|answer| {
                        (
                            answer.clarification_id.clone(),
                            answer.answer.trim().to_string(),
                        )
                    })
                    .filter(|(_, answer)| !answer.is_empty())
                    .collect::<BTreeMap<_, _>>()
                    != clarification_answers
                        .get()
                        .into_iter()
                        .map(|(id, answer)| (id, answer.trim().to_string()))
                        .filter(|(_, answer)| !answer.is_empty())
                        .collect::<BTreeMap<_, _>>()
        })
    });
    let missing_clarifications = Signal::derive(move || {
        let answers = clarification_answers.get();
        decision_workflow
            .get()
            .map(|workflow| {
                workflow
                    .clarifications
                    .iter()
                    .filter(|item| {
                        item.required
                            && answers
                                .get(&item.id)
                                .is_none_or(|answer| answer.trim().is_empty())
                    })
                    .count()
            })
            .unwrap_or_default()
    });
    let workflow_locked = Signal::derive(move || {
        workflow_busy.get()
            || project_busy.get()
            || version_saving.get()
            || status.get() == RunStatus::Running
            || controller.get().is_some()
            || pending_review.get().is_some()
    });
    let has_unsaved_changes = Signal::derive(move || {
        let workflow = decision_workflow.get();
        let saved_requirement = workflow
            .as_ref()
            .map(|workflow| workflow.requirement_text.as_str())
            .unwrap_or_default();
        let prompt_changed = if generation_mode.get() == GenerationMode::Modify {
            prompt.get() != prompt_checkpoint.get()
        } else {
            prompt.get() != saved_requirement
        };
        let source_dsl_changed = source_dsl.get() != source_dsl_checkpoint.get();
        prompt_changed
            || source_dsl_changed
            || pending_review.get().is_some()
            || workflow.as_ref().is_some_and(|workflow| {
                clarification_answers.get()
                    != workflow
                        .answers
                        .iter()
                        .map(|answer| (answer.clarification_id.clone(), answer.answer.clone()))
                        .collect::<BTreeMap<_, _>>()
            })
            || workflow
                .as_ref()
                .is_some_and(|workflow| workflow.status != "confirmed")
    });
    let can_model = Signal::derive(move || {
        let Some(project) = current_project.get() else {
            return false;
        };
        let username = user_id.get();
        user_role.get() == "admin"
            || project.owner_username == username
            || project_members.get().iter().any(|member| {
                member.project_id == project.id
                    && member.username == username
                    && member.role == "modeler"
            })
    });
    let can_review = Signal::derive(move || {
        let Some(project) = current_project.get() else {
            return false;
        };
        user_role.get() == "admin"
            || project_members.get().iter().any(|member| {
                member.project_id == project.id
                    && member.username == user_id.get()
                    && member.role == "reviewer"
            })
    });
    let can_manage_service = Signal::derive(move || {
        let Some(project) = current_project.get() else {
            return false;
        };
        let username = user_id.get();
        user_role.get() == "admin"
            || project.owner_username == username
            || project_members.get().iter().any(|member| {
                member.project_id == project.id
                    && member.username == username
                    && member.role.eq_ignore_ascii_case("approver")
            })
    });
    let app_state = expect_context::<AppState>();
    Effect::new(move |_| {
        if project_busy.get() {
            return;
        }
        let (Some(project), view) = (current_project.get(), workspace_view.get()) else {
            return;
        };
        let _ = LocalStorage::set(
            workspace_view_storage_key(project.id),
            workspace_view_key(view),
        );
    });
    let cleanup_app_state = app_state.clone();
    on_cleanup(move || cleanup_app_state.ui.workspace_dirty.set(false));
    let dirty_app_state = app_state.clone();
    Effect::new(move |_| {
        dirty_app_state
            .ui
            .workspace_dirty
            .set(has_unsaved_changes.get());
    });
    let local_draft = Signal::derive(move || {
        let Some(project) = current_project.get() else {
            return None;
        };
        let workflow = decision_workflow.get();
        let saved_requirement = workflow
            .as_ref()
            .map(|workflow| workflow.requirement_text.as_str())
            .unwrap_or_default();
        let prompt_changed = if generation_mode.get() == GenerationMode::Modify {
            prompt.get() != prompt_checkpoint.get()
        } else {
            prompt.get() != saved_requirement
        };
        let source_dsl_changed = source_dsl.get() != source_dsl_checkpoint.get();
        let saved_answers = workflow
            .as_ref()
            .map(|workflow| {
                workflow
                    .answers
                    .iter()
                    .map(|answer| (answer.clarification_id.clone(), answer.answer.clone()))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        let answers_changed = clarification_answers.get() != saved_answers;
        if !prompt_changed
            && !source_dsl_changed
            && !answers_changed
            && pending_review.get().is_none()
        {
            return None;
        }
        Some(WorkspaceDraftSnapshot {
            project_id: project.id,
            base_version_id: current_version.get().map(|version| version.id),
            prompt: prompt.get(),
            clarification_answers: clarification_answers.get(),
            pending_review: pending_review.get(),
            pending_candidate_id: pending_candidate_id.get(),
            pending_knowledge_citations: pending_knowledge_citations.get(),
            review_confirmed: review_confirmed.get(),
            dsl: dsl.get(),
            source_dsl: source_dsl.get(),
            xml: xml.get(),
            workspace_view: workspace_view_key(workspace_view.get()).into(),
            modify_mode: generation_mode.get() == GenerationMode::Modify,
            version_mismatch: false,
        })
    });
    let persistence_app_state = app_state.clone();
    Effect::new(move |_| {
        let Some(project) = current_project.get() else {
            return;
        };
        if project_busy.get() || draft_to_restore.get().is_some() {
            return;
        }
        if let Some(snapshot) = local_draft.get() {
            persistence_app_state.save_workspace_draft(&snapshot);
        } else {
            persistence_app_state.clear_workspace_draft(project.id);
        }
    });
    let cleanup_draft_app_state = app_state.clone();
    on_cleanup(move || {
        if draft_to_restore.get_untracked().is_some() {
            return;
        }
        let Some(project) = current_project.get_untracked() else {
            return;
        };
        if let Some(snapshot) = local_draft.get_untracked() {
            cleanup_draft_app_state.save_workspace_draft(&snapshot);
        } else if !project_busy.get_untracked() {
            cleanup_draft_app_state.clear_workspace_draft(project.id);
        }
    });
    let theme = app_state.ui.theme.read_only();
    provide_app_context(
        token.into(),
        user_role,
        theme,
        current_project,
        current_version,
    );
    // 在工作流尚未加载前展示需求分析；加载后根据是否存在方案切换到对应 Tab。
    let (mobile_model_focus, set_mobile_model_focus) = signal(MobileModelFocus::Conversation);
    let (conversation_width, set_conversation_width) = signal(380.0_f64);
    let (model_expanded, set_model_expanded) = signal(false);
    let (resizing_workspace, set_resizing_workspace) = signal(false);
    let workspace_grid = NodeRef::<leptos::html::Div>::new();
    let viewport_width = RwSignal::new(
        window()
            .inner_width()
            .ok()
            .and_then(|width| width.as_f64())
            .unwrap_or(1280.0),
    );
    let viewport_listener = window_event_listener(leptos::ev::resize, move |_| {
        if let Some(width) = window().inner_width().ok().and_then(|width| width.as_f64()) {
            viewport_width.set(width);
            if width <= 1100.0 {
                set_model_expanded.set(false);
                set_resizing_workspace.set(false);
            }
        }
    });
    on_cleanup(move || viewport_listener.remove());
    let resize_start = move |event: leptos::ev::MouseEvent| {
        if viewport_width.get_untracked() <= 1100.0 || model_expanded.get_untracked() {
            return;
        }
        event.prevent_default();
        set_resizing_workspace.set(true);
    };
    let resize_move_listener = window_event_listener(leptos::ev::mousemove, move |event| {
        if !resizing_workspace.get_untracked() {
            return;
        }
        let Some(grid) = workspace_grid.get_untracked() else {
            return;
        };
        let available_width = f64::from(grid.client_width());
        let grid_left = f64::from(grid.offset_left());
        let maximum_width = (available_width - 420.0).max(320.0).min(560.0);
        let next_width = (f64::from(event.client_x()) - grid_left).clamp(320.0, maximum_width);
        set_conversation_width.set(next_width);
    });
    on_cleanup(move || resize_move_listener.remove());
    let resize_end_listener = window_event_listener(leptos::ev::mouseup, move |_| {
        set_resizing_workspace.set(false);
    });
    on_cleanup(move || resize_end_listener.remove());
    let resize_keydown = move |event: KeyboardEvent| {
        if viewport_width.get_untracked() <= 1100.0 || model_expanded.get_untracked() {
            return;
        }
        let step = if event.shift_key() { 80.0 } else { 24.0 };
        let current = conversation_width.get_untracked();
        let maximum = workspace_grid
            .get_untracked()
            .map(|grid| {
                (f64::from(grid.client_width()) - 420.0)
                    .max(320.0)
                    .min(560.0)
            })
            .unwrap_or(560.0);
        let next = match event.key().as_str() {
            "ArrowLeft" => Some((current - step).max(320.0)),
            "ArrowRight" => Some((current + step).min(maximum)),
            "Home" => Some(320.0),
            "End" => Some(maximum),
            _ => None,
        };
        if let Some(next) = next {
            event.prevent_default();
            set_conversation_width.set(next);
        }
    };
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
    let follow_messages = RwSignal::new(true);
    let unread_messages = RwSignal::new(false);
    let test_drawer_body = NodeRef::<leptos::html::Div>::new();

    Effect::new(move |_| {
        let Some(token_value) = token.get() else {
            set_user_id.set(String::new());
            set_user_role.set(String::new());
            return;
        };
        spawn_local(async move {
            if let Some((username, role)) = api::me(&token_value).await {
                set_user_id.set(username);
                set_user_role.set(role);
            }
        });
    });

    let initial_app_state = app_state.clone();
    Effect::new(move |_| {
        let Some(token_value) = token.get() else {
            return;
        };
        set_project_busy.set(true);
        let load_app_state = initial_app_state.clone();
        spawn_local(async move {
            match api::list_projects(&token_value).await {
                Ok(items) => {
                    set_projects.set(items.clone());
                    let preferred_project_id =
                        initial_app_state.ui.active_project_id.get_untracked();
                    let project = items
                        .iter()
                        .find(|project| Some(project.id) == preferred_project_id)
                        .cloned()
                        .or_else(|| items.first().cloned());
                    if let Some(project) = project {
                        set_workspace_view.set(load_workspace_view(project.id));
                        match api::get_project_workspace(&token_value, project.id).await {
                            Ok(workspace) => {
                                load_app_state.set_active_project(Some(project.id));
                                set_draft_to_restore.set(load_compatible_workspace_draft(
                                    &load_app_state,
                                    project.id,
                                    workspace.version.as_ref().map(|version| version.id),
                                ));
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
                                set_source_dsl_checkpoint.set(source_dsl.get_untracked());
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
                                        let prompt_value = workflow
                                            .as_ref()
                                            .map(|state| state.requirement_text.clone())
                                            .unwrap_or_default();
                                        set_prompt_checkpoint.set(prompt_value.clone());
                                        set_prompt.set(prompt_value);
                                        set_decision_workflow.set(workflow);
                                    }
                                    Err(message) => {
                                        set_prompt_checkpoint.set(String::new());
                                        set_workflow_error.set(Some(message));
                                    }
                                }
                            }
                            Err(message) => set_project_error.set(Some(message)),
                        }
                    } else {
                        load_app_state.set_active_project(None);
                        set_draft_to_restore.set(None);
                    }
                }
                Err(message) => set_project_error.set(Some(message)),
            }
            set_project_busy.set(false);
        });
    });

    Effect::new(move |_| {
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            set_pending_requirement_review.set(None);
            set_selected_review_model.set(None);
            return;
        };
        spawn_local(async move {
            if let Ok(review) = api::get_pending_requirement_review(&token_value, project.id).await
                && current_project
                    .get_untracked()
                    .is_some_and(|current| current.id == project.id)
            {
                if review.is_some() {
                    set_workspace_view.set(WorkspaceView::Discovery);
                }
                set_pending_requirement_review.set(review);
                set_selected_review_model.set(None);
            }
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
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            set_analytics_thresholds.set(crate::models::DecisionAnalyticsThresholds::default());
            set_analytics_thresholds_loading.set(false);
            set_analytics_thresholds_error.set(None);
            return;
        };
        set_analytics_thresholds_loading.set(true);
        spawn_local(async move {
            match api::get_decision_analytics_thresholds(&token_value, project.id).await {
                Ok(thresholds)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_analytics_thresholds.set(thresholds);
                    set_analytics_thresholds_error.set(None);
                }
                Ok(_) => {}
                Err(message)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_analytics_thresholds_error.set(Some(message));
                }
                Err(_) => {}
            }
            if current_project
                .get_untracked()
                .is_some_and(|current| current.id == project.id)
            {
                set_analytics_thresholds_loading.set(false);
            }
        });
    });

    Effect::new(move |_| {
        let _ = analytics_revision.get();
        set_shadow_request_generation.update(|generation| {
            *generation = generation.wrapping_add(1);
        });
        let request_generation = shadow_request_generation.get_untracked();
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            set_shadow_evaluations.set(Vec::new());
            set_shadow_cursor.set(0);
            set_shadow_has_more.set(false);
            set_shadow_busy.set(false);
            set_shadow_error.set(None);
            set_optimization_proposals.set(Vec::new());
            set_drift_events.set(Vec::new());
            set_drift_scan_windows.set(Vec::new());
            set_deviation_analyses.set(Vec::new());
            set_drift_impacts.set(BTreeMap::new());
            set_drift_rule.set(None);
            set_proposal_error.set(None);
            set_drift_event_error.set(None);
            set_drift_rule_error.set(None);
            set_simulation_candidate_version_id.set(None);
            set_optimization_simulation.set(None);
            return;
        };
        set_simulation_candidate_version_id.set(None);
        set_optimization_simulation.set(None);
        set_shadow_evaluations.set(Vec::new());
        set_shadow_cursor.set(0);
        set_shadow_has_more.set(false);
        set_shadow_busy.set(false);
        set_shadow_error.set(None);
        if project.published_version_id.is_none() {
            set_optimization_proposals.set(Vec::new());
            set_drift_events.set(Vec::new());
            set_drift_scan_windows.set(Vec::new());
            set_deviation_analyses.set(Vec::new());
            set_drift_impacts.set(BTreeMap::new());
            set_drift_rule.set(None);
            set_proposal_error.set(None);
            set_drift_event_error.set(None);
            set_drift_rule_error.set(None);
            return;
        }
        set_shadow_busy.set(true);
        spawn_local(async move {
            match api::list_optimization_shadow_evaluations(&token_value, project.id, 0, 100).await
            {
                Ok(page)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id)
                        && shadow_request_generation.get_untracked() == request_generation =>
                {
                    set_shadow_cursor.set(page.last().map(|item| item.id).unwrap_or_default());
                    set_shadow_has_more.set(page.len() == 100);
                    set_shadow_evaluations.set(page);
                }
                Ok(_) => {}
                Err(message)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id)
                        && shadow_request_generation.get_untracked() == request_generation =>
                {
                    set_shadow_error.set(Some(message));
                }
                Err(_) => {}
            }
            if shadow_request_generation.get_untracked() == request_generation {
                set_shadow_busy.set(false);
            }
            match api::get_drift_automation_rule(&token_value, project.id).await {
                Ok(rule)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_drift_rule_enabled.set(rule.as_ref().is_some_and(|rule| rule.enabled));
                    set_drift_rule_codes.set(
                        rule.as_ref()
                            .map(|rule| rule.finding_codes.clone())
                            .unwrap_or_default(),
                    );
                    drift_rule_cooldown.set(
                        rule.as_ref()
                            .map(|rule| rule.cooldown_hours.to_string())
                            .unwrap_or_else(|| "24".into()),
                    );
                    set_drift_webhook_url.set(
                        rule.as_ref()
                            .and_then(|rule| rule.webhook_url.clone())
                            .unwrap_or_default(),
                    );
                    set_drift_rule.set(rule);
                }
                Ok(_) => {}
                Err(message)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_drift_rule_error.set(Some(message));
                }
                Err(_) => {}
            }
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
            match api::list_drift_events(&token_value, project.id).await {
                Ok(events)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_drift_events.set(events);
                }
                Ok(_) => {}
                Err(message)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_drift_event_error.set(Some(message));
                }
                Err(_) => {}
            }
            match api::list_drift_scan_windows(&token_value, project.id).await {
                Ok(windows)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_drift_scan_windows.set(windows);
                }
                Ok(_) => {}
                Err(message)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_drift_event_error.set(Some(message));
                }
                Err(_) => {}
            }
            match api::list_deviation_analyses(&token_value, project.id).await {
                Ok(analyses)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_deviation_analyses.set(analyses);
                }
                Ok(_) => {}
                Err(message)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_drift_event_error.set(Some(message));
                }
                Err(_) => {}
            }
        });
    });

    Effect::new(move |_| {
        let has_messages = messages.with(|messages| !messages.is_empty());
        let has_delta = assistant_delta.with(|delta| !delta.is_empty());
        let has_review = pending_review.with(|review| review.is_some());
        let has_content = has_messages || has_delta || has_review;
        let width = viewport_width.get_untracked();
        let focus = mobile_model_focus.get_untracked();
        if message_list.get().is_some() {
            if width <= 760.0 || (width <= 1100.0 && focus == MobileModelFocus::Inspector) {
                if has_content {
                    unread_messages.set(true);
                }
                return;
            }
            if follow_messages.get_untracked() {
                request_animation_frame(move || {
                    if follow_messages.try_get_untracked() == Some(true)
                        && let Some(Some(element)) = message_list.try_get_untracked()
                        && element.client_height() > 0
                    {
                        element.set_scroll_top(element.scroll_height());
                        unread_messages.set(false);
                    }
                });
            } else {
                unread_messages.set(true);
            }
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
            set_test_case_filter.set(TestCaseFilter::All);
        }
        let request_revision = test_case_load_revision.get();
        set_test_case_error.set(None);
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
                    let pending_count = proposals
                        .iter()
                        .filter(|proposal| proposal.status == "open")
                        .count();
                    let proposal_count = proposals.len();
                    set_optimization_proposals.set(proposals);
                    if pending_count > 0 {
                        set_version_save_status.set(Some(format!(
                            "已生成 {pending_count} 条待确认优化建议，确认操作位于下方建议列表"
                        )));
                        scroll_to_workflow_anchor("optimization-proposals");
                    } else if proposal_count == 0 {
                        set_version_save_status
                            .set(Some("当前发布版本未发现需要人工确认的优化建议".into()));
                    } else {
                        set_version_save_status.set(Some(
                            "当前没有待确认优化建议，已有建议均已处理或撤回".into(),
                        ));
                    }
                    set_analytics_revision.update(|revision| *revision += 1);
                    set_audit_revision.update(|revision| *revision += 1);
                }
                Err(message) => set_proposal_error.set(Some(message)),
            }
            set_proposal_busy.set(false);
        });
    };

    let save_analytics_thresholds = move |_| {
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            return;
        };
        if analytics_thresholds_busy.get() {
            return;
        }
        let thresholds = analytics_thresholds.get();
        set_analytics_thresholds_busy.set(true);
        set_analytics_thresholds_error.set(None);
        spawn_local(async move {
            match api::update_decision_analytics_thresholds(&token_value, project.id, &thresholds)
                .await
            {
                Ok(saved)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_analytics_thresholds.set(saved);
                    set_analytics_revision.update(|revision| *revision += 1);
                    set_audit_revision.update(|revision| *revision += 1);
                }
                Ok(_) => {}
                Err(message)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_analytics_thresholds_error.set(Some(message));
                }
                Err(_) => {}
            }
            if current_project
                .get_untracked()
                .is_some_and(|current| current.id == project.id)
            {
                set_analytics_thresholds_busy.set(false);
            }
        });
    };

    let prepare_drift_event_proposals = Callback::new(move |event_id: i64| {
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            return;
        };
        if drift_event_busy.get() {
            return;
        }
        set_drift_event_busy.set(true);
        set_drift_event_error.set(None);
        spawn_local(async move {
            match api::prepare_drift_event_proposals(&token_value, project.id, event_id).await {
                Ok(result)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_drift_events.update(|events| {
                        if let Some(event) = events.iter_mut().find(|event| event.id == event_id) {
                            event.status = result.status.clone();
                            event.action_result = Some(serde_json::json!({
                                "proposalIds": result.proposal_ids.clone()
                            }));
                        }
                    });
                    set_version_save_status.set(Some(format!(
                        "漂移事件已生成 {} 条待确认提案；请继续模拟并人工确认",
                        result.proposal_ids.len()
                    )));
                    set_analytics_revision.update(|revision| *revision += 1);
                    set_audit_revision.update(|revision| *revision += 1);
                }
                Ok(_) => {}
                Err(message) => set_drift_event_error.set(Some(message)),
            }
            set_drift_event_busy.set(false);
        });
    });

    let load_drift_event_impact = Callback::new(move |event_id: i64| {
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            return;
        };
        if drift_impact_busy.get().is_some() {
            return;
        }
        set_drift_impact_busy.set(Some(event_id));
        set_drift_event_error.set(None);
        spawn_local(async move {
            match api::get_drift_event_impact(&token_value, project.id, event_id).await {
                Ok(report)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_drift_impacts.update(|reports| {
                        reports.insert(event_id, report);
                    });
                }
                Ok(_) => {}
                Err(message)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_drift_event_error.set(Some(message));
                }
                Err(_) => {}
            }
            set_drift_impact_busy.set(None);
        });
    });

    let save_drift_automation_rule = move |_| {
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            return;
        };
        if drift_rule_busy.get() || !can_model.get() {
            return;
        }
        let Ok(cooldown_hours) = drift_rule_cooldown.get().parse::<i32>() else {
            set_drift_rule_error.set(Some("冷却时间必须是 1 到 720 之间的整数".into()));
            return;
        };
        let payload = UpdateDecisionDriftAutomation {
            enabled: drift_rule_enabled.get(),
            finding_codes: drift_rule_codes.get(),
            cooldown_hours,
            webhook_url: Some(drift_webhook_url.get()),
        };
        set_drift_rule_busy.set(true);
        set_drift_rule_error.set(None);
        spawn_local(async move {
            match api::update_drift_automation_rule(&token_value, project.id, &payload).await {
                Ok(rule)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_drift_rule_enabled.set(rule.enabled);
                    set_drift_rule_codes.set(rule.finding_codes.clone());
                    drift_rule_cooldown.set(rule.cooldown_hours.to_string());
                    set_drift_webhook_url.set(rule.webhook_url.clone().unwrap_or_default());
                    set_drift_rule.set(Some(rule));
                    set_version_save_status.set(Some("漂移扫描规则已保存".into()));
                    set_audit_revision.update(|revision| *revision += 1);
                }
                Ok(_) => {}
                Err(message) => set_drift_rule_error.set(Some(message)),
            }
            set_drift_rule_busy.set(false);
        });
    };

    let dismiss_drift_event = Callback::new(move |event_id: i64| {
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            return;
        };
        if drift_event_busy.get() || !can_model.get() {
            return;
        }
        set_drift_event_busy.set(true);
        set_drift_event_error.set(None);
        spawn_local(async move {
            match api::dismiss_drift_event(&token_value, project.id, event_id).await {
                Ok(result)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_drift_events.update(|events| {
                        if let Some(event) = events.iter_mut().find(|event| event.id == event_id) {
                            event.status = result.status.clone();
                            event.action_result = Some(serde_json::json!({"action": "dismissed"}));
                        }
                    });
                    set_version_save_status.set(Some("漂移事件已忽略并记录审计".into()));
                    set_audit_revision.update(|revision| *revision += 1);
                }
                Ok(_) => {}
                Err(message) => set_drift_event_error.set(Some(message)),
            }
            set_drift_event_busy.set(false);
        });
    });

    let review_drift_event = Callback::new(move |(event_id, review_label): (i64, String)| {
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            return;
        };
        if drift_event_busy.get() || !can_model.get() {
            return;
        }
        let payload = UpdateDecisionDriftEventReview { review_label };
        set_drift_event_busy.set(true);
        set_drift_event_error.set(None);
        spawn_local(async move {
            match api::review_drift_event(&token_value, project.id, event_id, &payload).await {
                Ok(review)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_drift_events.update(|events| {
                        if let Some(event) = events.iter_mut().find(|event| event.id == event_id) {
                            event.review_label = Some(review.review_label);
                            event.reviewed_by = Some(review.reviewed_by);
                            event.reviewed_at = Some(review.reviewed_at);
                        }
                    });
                    set_version_save_status.set(Some("漂移事件人工判定已保存并记录审计".into()));
                    set_audit_revision.update(|revision| *revision += 1);
                }
                Ok(_) => {}
                Err(message) => set_drift_event_error.set(Some(message)),
            }
            set_drift_event_busy.set(false);
        });
    });

    let review_drift_scan_window =
        Callback::new(move |(window_id, review_label): (i64, String)| {
            let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
                return;
            };
            if drift_event_busy.get() || !can_model.get() {
                return;
            }
            let payload = UpdateDecisionDriftScanWindowReview { review_label };
            set_drift_event_busy.set(true);
            set_drift_event_error.set(None);
            spawn_local(async move {
                match api::review_drift_scan_window(&token_value, project.id, window_id, &payload)
                    .await
                {
                    Ok(review)
                        if current_project
                            .get_untracked()
                            .is_some_and(|current| current.id == project.id) =>
                    {
                        set_drift_scan_windows.update(|windows| {
                            if let Some(window) =
                                windows.iter_mut().find(|window| window.id == window_id)
                            {
                                window.review_label = Some(review.review_label);
                                window.reviewed_by = Some(review.reviewed_by);
                                window.reviewed_at = Some(review.reviewed_at);
                            }
                        });
                        set_version_save_status
                            .set(Some("漂移扫描窗口人工判定已保存并记录审计".into()));
                        set_audit_revision.update(|revision| *revision += 1);
                    }
                    Ok(_) => {}
                    Err(message) => set_drift_event_error.set(Some(message)),
                }
                set_drift_event_busy.set(false);
            });
        });

    let review_deviation_analysis =
        Callback::new(move |(analysis_id, review_label): (i64, String)| {
            let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
                return;
            };
            if drift_event_busy.get() || !can_model.get() {
                return;
            }
            let payload = UpdateDeviationAnalysisReview { review_label };
            set_drift_event_busy.set(true);
            set_drift_event_error.set(None);
            spawn_local(async move {
                match api::review_deviation_analysis(
                    &token_value,
                    project.id,
                    analysis_id,
                    &payload,
                )
                .await
                {
                    Ok(review)
                        if current_project
                            .get_untracked()
                            .is_some_and(|current| current.id == project.id) =>
                    {
                        set_deviation_analyses.update(|analyses| {
                            if let Some(analysis) =
                                analyses.iter_mut().find(|item| item.id == analysis_id)
                            {
                                analysis.review_label = Some(review.review_label);
                                analysis.reviewed_by = Some(review.reviewed_by);
                                analysis.reviewed_at = Some(review.reviewed_at);
                            }
                        });
                        set_version_save_status
                            .set(Some("偏差归因人工判定已保存并记录审计".into()));
                        set_audit_revision.update(|revision| *revision += 1);
                    }
                    Ok(_) => {}
                    Err(message) => set_drift_event_error.set(Some(message)),
                }
                set_drift_event_busy.set(false);
            });
        });

    let retry_drift_webhook = Callback::new(move |event_id: i64| {
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            return;
        };
        if drift_event_busy.get() || !can_model.get() {
            return;
        }
        set_drift_event_busy.set(true);
        set_drift_event_error.set(None);
        spawn_local(async move {
            match api::retry_drift_webhook_delivery(&token_value, project.id, event_id).await {
                Ok(_)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    set_drift_events.update(|events| {
                        if let Some(event) = events.iter_mut().find(|event| event.id == event_id) {
                            event.webhook_delivery =
                                Some(serde_json::json!({ "status": "pending", "attemptCount": 0 }));
                        }
                    });
                    set_version_save_status.set(Some("漂移通知已重新加入投递队列".into()));
                    set_audit_revision.update(|revision| *revision += 1);
                }
                Ok(_) => {}
                Err(message) => set_drift_event_error.set(Some(message)),
            }
            set_drift_event_busy.set(false);
        });
    });

    let load_more_shadow_evaluations = Callback::new(move |_| {
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            return;
        };
        if shadow_busy.get() || !shadow_has_more.get() {
            return;
        }
        let after_id = shadow_cursor.get();
        set_shadow_request_generation.update(|generation| {
            *generation = generation.wrapping_add(1);
        });
        let request_generation = shadow_request_generation.get_untracked();
        set_shadow_busy.set(true);
        set_shadow_error.set(None);
        spawn_local(async move {
            match api::list_optimization_shadow_evaluations(&token_value, project.id, after_id, 100)
                .await
            {
                Ok(page)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id)
                        && shadow_request_generation.get_untracked() == request_generation =>
                {
                    set_shadow_cursor.set(page.last().map(|item| item.id).unwrap_or(after_id));
                    set_shadow_has_more.set(page.len() == 100);
                    set_shadow_evaluations.update(|items| items.extend(page));
                }
                Ok(_) => {}
                Err(message)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id)
                        && shadow_request_generation.get_untracked() == request_generation =>
                {
                    set_shadow_error.set(Some(message));
                }
                Err(_) => {}
            }
            if current_project
                .get_untracked()
                .is_some_and(|current| current.id == project.id)
                && shadow_request_generation.get_untracked() == request_generation
            {
                set_shadow_busy.set(false);
            }
        });
    });

    let simulate_optimization = Callback::new(move |proposal_id: i64| {
        let (Some(token_value), Some(project), Some(candidate_version_id)) = (
            token.get(),
            current_project.get(),
            simulation_candidate_version_id.get(),
        ) else {
            set_proposal_error.set(Some("请先选择候选版本再运行效果模拟".into()));
            return;
        };
        if simulation_busy.get() {
            return;
        }
        set_simulation_busy.set(true);
        set_proposal_error.set(None);
        set_optimization_simulation.set(None);
        spawn_local(async move {
            match api::simulate_optimization_proposal(
                &token_value,
                project.id,
                proposal_id,
                candidate_version_id,
            )
            .await
            {
                Ok(report)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project.id) =>
                {
                    if report.proposal_withdrawn {
                        let resolution_note = report.estimated_business_impact.clone();
                        set_optimization_proposals.update(|proposals| {
                            if let Some(proposal) =
                                proposals.iter_mut().find(|item| item.id == proposal_id)
                            {
                                proposal.status = "withdrawn".into();
                                proposal.resolution_note = resolution_note;
                            }
                        });
                    }
                    set_optimization_proposals.update(|proposals| {
                        if let Some(proposal) =
                            proposals.iter_mut().find(|item| item.id == proposal_id)
                        {
                            proposal.latest_simulation_passed = report.passed;
                        }
                    });
                    if let Some(created_version_id) = report.autodraft_created_version_id {
                        match api::get_project_workspace(&token_value, project.id).await {
                            Ok(workspace)
                                if workspace.version.as_ref().map(|version| version.id)
                                    == Some(created_version_id)
                                    && current_project
                                        .get_untracked()
                                        .is_some_and(|current| current.id == project.id) =>
                            {
                                let version = workspace.version.clone();
                                set_current_project.set(Some(workspace.project));
                                set_current_version.set(version);
                                set_dsl.set(workspace.dsl_yaml.clone());
                                set_source_dsl.set(workspace.dsl_yaml.unwrap_or_default());
                                set_source_dsl_checkpoint.set(source_dsl.get_untracked());
                                set_xml.set(workspace.dmn_xml);
                                set_generation_mode.set(GenerationMode::Modify);
                                set_pending_review.set(None);
                                set_review_confirmed.set(false);
                                set_verification_report.set(None);
                                set_publish_confirmed.set(false);
                                set_workspace_view.set(WorkspaceView::Model);
                                set_test_case_load_revision.update(|revision| *revision += 1);
                                set_version_save_status.set(Some(
                                    "自动草稿已创建：请完成业务验收、独立复核和人工发布".into(),
                                ));
                                match api::list_project_versions(&token_value, project.id).await {
                                    Ok(versions) => set_versions.set(versions),
                                    Err(message) => set_project_error.set(Some(message)),
                                }
                                match api::list_optimization_proposals(&token_value, project.id)
                                    .await
                                {
                                    Ok(proposals) => set_optimization_proposals.set(proposals),
                                    Err(message) => set_proposal_error.set(Some(message)),
                                }
                                set_analytics_revision.update(|revision| *revision += 1);
                                set_audit_revision.update(|revision| *revision += 1);
                            }
                            Ok(_) => {}
                            Err(message) => set_project_error.set(Some(message)),
                        }
                    }
                    set_optimization_simulation.set(Some(report));
                    set_shadow_evaluations.set(Vec::new());
                    set_shadow_cursor.set(0);
                    set_shadow_request_generation.update(|generation| {
                        *generation = generation.wrapping_add(1);
                    });
                    let request_generation = shadow_request_generation.get_untracked();
                    set_shadow_busy.set(true);
                    match api::list_optimization_shadow_evaluations(
                        &token_value,
                        project.id,
                        0,
                        100,
                    )
                    .await
                    {
                        Ok(page)
                            if current_project
                                .get_untracked()
                                .is_some_and(|current| current.id == project.id)
                                && shadow_request_generation.get_untracked()
                                    == request_generation =>
                        {
                            set_shadow_cursor
                                .set(page.last().map(|item| item.id).unwrap_or_default());
                            set_shadow_has_more.set(page.len() == 100);
                            set_shadow_evaluations.set(page);
                        }
                        Ok(_) => {}
                        Err(message)
                            if current_project
                                .get_untracked()
                                .is_some_and(|current| current.id == project.id)
                                && shadow_request_generation.get_untracked()
                                    == request_generation =>
                        {
                            set_shadow_error.set(Some(message));
                        }
                        Err(_) => {}
                    }
                    if shadow_request_generation.get_untracked() == request_generation {
                        set_shadow_busy.set(false);
                    }
                }
                Ok(_) => {}
                Err(message) => set_proposal_error.set(Some(message)),
            }
            set_simulation_busy.set(false);
        });
    });

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
                    set_source_dsl_checkpoint.set(source_dsl.get_untracked());
                    set_xml.set(workspace.dmn_xml);
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
        if !can_model.get() {
            return;
        }
        if workflow_locked.get() {
            return;
        }
        let base_revision = current_version.get().map(|version| version.id);
        let mode = if base_revision.is_some() {
            GenerationMode::Modify
        } else {
            GenerationMode::Create
        };
        let Some(workflow) = decision_workflow.get() else {
            set_workspace_view.set(WorkspaceView::Discovery);
            set_workflow_error.set(Some("请先完成需求分析和方案规划".into()));
            return;
        };
        if !workflow_current.get() || workflow_answers_changed.get() {
            set_workspace_view.set(WorkspaceView::Discovery);
            set_workflow_error.set(Some(
                "需求、回答或模型基线已变化，请重新分析或规划并确认方案".into(),
            ));
            return;
        }
        let simple_direct = simple_direct_workflow_eligible(&workflow);
        if !simple_direct {
            if let Err(message) = workflow.generation_prompt(base_revision) {
                set_workspace_view.set(WorkspaceView::Discovery);
                set_workflow_error.set(Some(message));
                return;
            }
        }
        let workflow_updated_at = workflow.updated_at;
        set_generation_mode.set(mode);
        let prompt_text = prompt.get();
        let user_text = if prompt_text.trim().is_empty() && mode == GenerationMode::Create {
            decision_workflow
                .get()
                .map(|workflow| workflow.requirement_text.trim().to_string())
                .unwrap_or_default()
        } else {
            prompt_text.trim().to_string()
        };
        if user_text.is_empty()
            || status.get() == RunStatus::Running
            || controller.get().is_some()
            || project_busy.get()
        {
            return;
        }
        let Some(token_value) = token.get() else {
            return;
        };
        let Some(project_id) = current_project.get().map(|project| project.id) else {
            set_error.set(Some("请先创建或选择一个决策项目".to_string()));
            return;
        };
        let retry_of_orchestration_id = engineer_retry_id.get();
        set_engineer_retry_id.set(None);
        set_active_orchestration_id.set(None);
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
            set_pending_knowledge_citations.set(Vec::new());
            set_review_confirmed.set(false);
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
            set_messages.update(|items| {
                items.push(ChatMessage {
                    role: "user",
                    content: user_text.clone(),
                })
            });
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
            let set_engineer_retry_id = set_engineer_retry_id;
            let set_active_orchestration_id = set_active_orchestration_id;
            spawn_local(async move {
                let change_request = "根据已确认的变更方案生成模型变更".to_string();
                let mut repair_attempt = 0;
                let mut repair_error = None;
                let mut previous_intent: Option<ModelChangeIntent> = None;
                let result = loop {
                    let mut generated_intent = None;
                    let mut current_orchestration_id = None;
                    let mut stream_error = None;
                    let intent_stream_result = api::generate_change_intent_stream(
                        &token_value,
                        project_id,
                        change_request.clone(),
                        base.id,
                        workflow_updated_at,
                        repair_error.clone(),
                        repair_attempt,
                        previous_intent.clone(),
                        retry_of_orchestration_id,
                        controller_value.clone(),
                        |event| match event {
                            SseEvent::Step(message) => {
                                set_agent_activity.set(agent_activity_for_step(&message));
                                set_steps.update(|items| items.push(message));
                            }
                            SseEvent::ChangeIntent(intent) => generated_intent = Some(intent),
                            SseEvent::Orchestration(id) => {
                                current_orchestration_id = Some(id);
                                set_active_orchestration_id.set(Some(id));
                            }
                            SseEvent::Error(message) => {
                                set_engineer_retry_id.set(None);
                                stream_error = Some(message.clone());
                                set_steps.update(|items| {
                                    items.push(format!("⚠ 变更意图生成失败：{message}"))
                                });
                                set_agent_activity.set("变更意图生成失败".into());
                            }
                            SseEvent::RetryableFailure { message, retryable } => {
                                stream_error = Some(message.clone());
                                let retry_root = retry_of_orchestration_id
                                    .or(current_orchestration_id)
                                    .filter(|id| *id > 0);
                                set_engineer_retry_id.set(if retryable {
                                    retry_root
                                } else {
                                    None
                                });
                                set_steps.update(|items| {
                                    items.push(format!("⚠ 变更意图生成失败：{message}"))
                                });
                                set_agent_activity.set("变更意图生成失败".into());
                            }
                            SseEvent::Delta(_)
                            | SseEvent::Preview(_)
                            | SseEvent::Result(_)
                            | SseEvent::WorkflowResult(_) => {}
                            event @ (SseEvent::Agent(_)
                            | SseEvent::Artifact(_)
                            | SseEvent::Handoff(_)
                            | SseEvent::Stage(_)) => {
                                if let Some((message, record)) = orchestration_event_update(&event)
                                {
                                    set_agent_activity.set(message.clone());
                                    if record {
                                        set_steps.update(|items| items.push(message));
                                    }
                                }
                            }
                        },
                    )
                    .await;
                    if controller_value.signal().aborted() {
                        set_controller.set(None);
                        set_active_orchestration_id.set(None);
                        set_status.set(RunStatus::Aborted);
                        return;
                    }
                    if let Err(error) = intent_stream_result {
                        break Err(error);
                    }
                    let intent = match generated_intent {
                        Some(intent) => intent,
                        None => {
                            break Err(
                                stream_error.unwrap_or_else(|| "变更意图流未返回有效结果".into())
                            );
                        }
                    };
                    set_steps
                        .update(|items| items.push("✓ 变更意图生成完成，正在校验 DSL...".into()));
                    set_agent_activity.set("正在校验模型变更意图".into());
                    match api::preview_model_change(&token_value, project_id, intent.clone()).await
                    {
                        Ok(candidate) => break Ok(candidate),
                        Err(error) if is_repairable_model_change_error(&error, repair_attempt) => {
                            repair_attempt += 1;
                            set_steps.update(|items| {
                                items.push(format!(
                                    "⚠ 模型变更校验未通过，正在修正变更意图（{repair_attempt}/{MAX_MODEL_CHANGE_REPAIRS}）..."
                                ));
                            });
                            set_agent_activity.set(format!(
                                "正在修正模型变更并重新校验（{repair_attempt}/{MAX_MODEL_CHANGE_REPAIRS}）"
                            ));
                            repair_error = Some(error);
                            previous_intent = Some(intent);
                        }
                        Err(error) => break Err(error),
                    }
                };
                match result {
                    Ok(candidate) => {
                        set_engineer_retry_id.set(None);
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
                set_active_orchestration_id.set(None);
            });
            return;
        }
        let text = "根据已确认方案生成 DSL 并编译 DMN".to_string();
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
        let request_for_retry = user_text.clone();
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
        set_engineer_retry_id.set(None);
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
        let set_engineer_retry_id = set_engineer_retry_id;
        let set_active_orchestration_id = set_active_orchestration_id;
        spawn_local(async move {
            let mut current_orchestration_id = None;
            let result = api::run_pipeline(
                &token_value,
                project_id,
                text,
                existing,
                base_version,
                workflow_updated_at,
                retry_of_orchestration_id,
                controller_value.clone(),
                |event| match event {
                    SseEvent::Orchestration(id) => {
                        current_orchestration_id = Some(id);
                        set_active_orchestration_id.set(Some(id));
                    }
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
                        if result.ok {
                            set_engineer_retry_id.set(None);
                            set_pending_knowledge_citations.set(result.knowledge_citations.clone());
                            set_agent_activity.set("模型已生成，可继续核验或测试".to_string());
                            set_steps.update(|items| items.push("✓ DMN XML 生成完成".to_string()));
                            if result.simple_direct_preview {
                                set_steps.update(|items| {
                                    items.push(
                                        "simple 候选基于未确认的需求卡结构生成；保存草稿前请人工检查".into(),
                                    );
                                });
                            }
                            if let Some(probes) = result
                                .tester_static_analysis
                                .as_ref()
                                .and_then(|report| report["executionProbes"].as_array())
                            {
                                let probe_count = probes.len();
                                set_steps.update(|items| {
                                    items.push(format!(
                                        "✓ Tester 已执行 {probe_count} 个静态发现见证探测，结果待人工确认"
                                    ));
                                });
                                for probe in probes.iter().take(5) {
                                    let finding = probe["findingCode"].as_str().unwrap_or("静态发现");
                                    let element = probe["element"].as_str().unwrap_or("决策");
                                    let status = probe["executionStatus"].as_str().unwrap_or("未知");
                                    let observed = probe["observedDecisions"]
                                        .as_array()
                                        .and_then(|decisions| decisions.first())
                                        .map(|decision| decision["observedOutput"].to_string())
                                        .unwrap_or_else(|| "无输出".to_string());
                                    let matched_rules = probe["observedDecisions"]
                                        .as_array()
                                        .and_then(|decisions| decisions.first())
                                        .and_then(|decision| decision["matchedRuleIds"].as_array())
                                        .map(|rules| {
                                            rules
                                                .iter()
                                                .filter_map(serde_json::Value::as_str)
                                                .collect::<Vec<_>>()
                                                .join(", ")
                                        })
                                        .filter(|rules| !rules.is_empty())
                                        .unwrap_or_else(|| "无命中规则".to_string());
                                    set_steps.update(|items| {
                                        items.push(format!(
                                            "Tester 探测 {element} / {finding}：{status}；观测输出 {observed}；{matched_rules}"
                                        ));
                                    });
                                }
                                if probe_count > 5 {
                                    set_steps.update(|items| {
                                        items.push(format!("其余 {} 个 Tester 探测详见运行结果", probe_count - 5));
                                    });
                                }
                            }
                            if result.dsl.is_some() && result.xml.is_some() {
                                set_pending_review.set(result.change_review.clone());
                                set_review_confirmed.set(false);
                                set_version_save_status
                                    .set(Some("模型已生成，等待确认变更后保存版本".into()));
                            }
                            if let Some(degradation) = result.degradation.as_ref() {
                                let reason =
                                    degradation["reason"].as_str().unwrap_or("Tester 未执行");
                                set_steps.update(|items| {
                                    items.push(format!("需人工复核：{reason}"));
                                });
                                set_version_save_status.set(Some(
                                    "Tester 已跳过或不可用，保存前请人工复核模型".into(),
                                ));
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
                        set_engineer_retry_id.set(None);
                        set_assistant_delta.set(String::new());
                        set_prompt.set(request_for_retry.clone());
                        set_agent_activity.set("生成失败，请修正需求后重试".to_string());
                        set_error.set(Some(message));
                        set_status.set(RunStatus::Failed);
                    }
                    SseEvent::RetryableFailure { message, retryable } => {
                        let retry_root = retry_of_orchestration_id
                            .or(current_orchestration_id)
                            .filter(|id| *id > 0);
                        set_engineer_retry_id.set(if retryable { retry_root } else { None });
                        set_assistant_delta.set(String::new());
                        set_prompt.set(request_for_retry.clone());
                        set_agent_activity.set("生成失败，请修正需求后重试".to_string());
                        set_error.set(Some(message));
                        set_status.set(RunStatus::Failed);
                    }
                    SseEvent::WorkflowResult(_) => {}
                    SseEvent::ChangeIntent(_) => {}
                    event @ (SseEvent::Agent(_)
                    | SseEvent::Artifact(_)
                    | SseEvent::Handoff(_)
                    | SseEvent::Stage(_)) => {
                        if let Some((message, record)) = orchestration_event_update(&event) {
                            set_agent_activity.set(message.clone());
                            if record {
                                set_steps.update(|items| items.push(message));
                            }
                        }
                    }
                },
            )
            .await;

            if controller_value.signal().aborted() {
                set_controller.set(None);
                set_active_orchestration_id.set(None);
                set_status.set(RunStatus::Aborted);
                set_agent_activity.set("已停止当前生成任务".to_string());
                return;
            }
            if let Err(message) = result {
                set_engineer_retry_id.set(None);
                set_assistant_delta.set(String::new());
                set_prompt.set(request_for_retry);
                set_agent_activity.set("请求未完成，请检查错误信息后重试".to_string());
                set_error.set(Some(message));
                set_status.set(RunStatus::Failed);
            }
            set_controller.set(None);
            set_active_orchestration_id.set(None);
        });
    };

    let confirm_change = move |_| {
        if !can_model.get() {
            return;
        }
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
                    set_versions.update(|items| {
                        items.retain(|item| item.id != version.id);
                        items.insert(0, version);
                    });
                    set_dsl.set(Some(submitted_dsl.clone()));
                    set_source_dsl.set(submitted_dsl.clone());
                    set_source_dsl_checkpoint.set(submitted_dsl.clone());
                    set_xml.set(Some(submitted_xml.clone()));
                    set_test_xml.set(submitted_xml.clone());
                    set_test_result.set(None);
                    set_test_error.set(None);
                    set_validation_result.set(None);
                    set_validation_error.set(None);
                    set_pending_review.set(None);
                    set_pending_candidate_id.set(None);
                    set_review_confirmed.set(true);
                    set_prompt_checkpoint.set(prompt.get_untracked());
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
                            set_source_dsl_checkpoint.set(source_dsl.get_untracked());
                            set_xml.set(workspace.dmn_xml.clone());
                            set_test_xml.set(workspace.dmn_xml.unwrap_or_default());
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
                    match api::get_decision_workflow(&token_value, project.id).await {
                        Ok(workflow) => set_decision_workflow.set(workflow),
                        Err(message) => set_workflow_error.set(Some(message)),
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
                    set_source_dsl_checkpoint.set(source_dsl.get_untracked());
                    set_xml.set(workspace.dmn_xml.clone());
                    set_test_xml.set(workspace.dmn_xml.unwrap_or_default());
                    set_test_result.set(None);
                    set_test_error.set(None);
                    set_validation_result.set(None);
                    set_validation_error.set(None);
                    set_steps.set(Vec::new());
                    set_assistant_delta.set(String::new());
                    set_agent_activity.set("等待新的建模任务".into());
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
            if let (Some(project), Some(token_value), Some(orchestration_id)) = (
                current_project.get(),
                token.get(),
                active_orchestration_id.get(),
            ) {
                let set_steps = set_steps;
                spawn_local(async move {
                    let cancel_result =
                        api::cancel_agent_orchestration(&token_value, project.id, orchestration_id)
                            .await;
                    active.abort();
                    if let Err(message) = cancel_result {
                        set_steps.update(|items| {
                            items.push(format!("服务端取消未确认：{message}；当前流已在本地停止"));
                        });
                    }
                });
            } else {
                active.abort();
            }
            set_active_orchestration_id.set(None);
            set_status.set(RunStatus::Aborted);
            set_agent_activity.set("已停止当前生成任务".to_string());
        }
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
            match api::get_pattern_library_analytics(&token_value).await {
                Ok(analytics) => set_pattern_analytics.set(Some(analytics)),
                Err(message) => set_model_library_error.set(Some(message)),
            }
            set_model_library_busy.set(false);
        });
    };

    let toggle_knowledge_library = move |_| {
        set_workspace_view.set(WorkspaceView::Resources);
        set_resource_view.set(ResourceView::Knowledge);
    };

    let load_pattern_lineage = move |version_id: i64| {
        let Some(token_value) = token.get() else {
            return;
        };
        set_pattern_lineage_busy.set(true);
        set_model_library_error.set(None);
        spawn_local(async move {
            match api::list_decision_pattern_lineage(&token_value, version_id).await {
                Ok(items) => set_pattern_lineage.set(items),
                Err(message) => set_model_library_error.set(Some(message)),
            }
            set_pattern_lineage_busy.set(false);
        });
    };

    let select_model_library_item = move |source_kind: String, source_id: String, name: String| {
        set_selected_model_source.set(Some(format!("{source_kind}:{source_id}")));
        set_reuse_project_name.set(name);
        set_reuse_confirmed.set(false);
        set_template_name.set(String::new());
        set_template_industry.set(String::new());
        set_pattern_lineage.set(Vec::new());
        set_model_library_error.set(None);
        if source_kind == "history"
            && let Ok(version_id) = source_id.parse::<i64>()
        {
            load_pattern_lineage(version_id);
        }
    };

    let request_pattern_recommendations = move |_| {
        let query = model_library_query.get().trim().to_string();
        if query.is_empty() || pattern_recommendations_busy.get() {
            return;
        }
        let Some(token_value) = token.get() else {
            return;
        };
        set_pattern_recommendations_busy.set(true);
        set_model_library_error.set(None);
        let project_id = current_project.get().map(|project| project.id);
        spawn_local(async move {
            match api::recommend_decision_patterns(&token_value, &query, project_id).await {
                Ok(items) => set_pattern_recommendations.set(items),
                Err(message) => set_model_library_error.set(Some(message)),
            }
            set_pattern_recommendations_busy.set(false);
        });
    };

    let recommend_current_requirement = move |_| {
        let query = prompt.get().trim().to_string();
        if query.is_empty() || pattern_recommendations_busy.get() {
            return;
        }
        let Some(token_value) = token.get() else {
            return;
        };
        set_pattern_recommendations_busy.set(true);
        set_model_library_error.set(None);
        let project_id = current_project.get().map(|project| project.id);
        spawn_local(async move {
            match api::recommend_decision_patterns(&token_value, &query, project_id).await {
                Ok(items) => set_pattern_recommendations.set(items),
                Err(message) => set_model_library_error.set(Some(message)),
            }
            set_pattern_recommendations_busy.set(false);
        });
    };

    let submit_pattern_feedback = move |recommendation_id: i64, accepted: bool| {
        if pattern_feedback_busy.get() {
            return;
        }
        let Some(token_value) = token.get() else {
            return;
        };
        set_pattern_feedback_busy.set(true);
        spawn_local(async move {
            if let Err(message) = api::submit_pattern_recommendation_feedback(
                &token_value,
                recommendation_id,
                &PatternRecommendationFeedback { accepted },
            )
            .await
            {
                set_model_library_error.set(Some(message));
            }
            set_pattern_feedback_busy.set(false);
        });
    };

    let instantiate_selected_model = Callback::new({
        let instantiate_app_state = app_state.clone();
        move |_: leptos::ev::MouseEvent| {
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
            let run_app_state = instantiate_app_state.clone();
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
                        run_app_state.set_active_project(Some(project.id));
                        set_projects.update(|items| items.insert(0, project.clone()));
                        set_current_project.set(Some(project));
                        set_current_version.set(version.clone());
                        set_dsl.set(workspace.dsl_yaml.clone());
                        set_source_dsl.set(workspace.dsl_yaml.unwrap_or_default());
                        set_source_dsl_checkpoint.set(source_dsl.get_untracked());
                        set_xml.set(workspace.dmn_xml);
                        set_generation_mode.set(GenerationMode::Modify);
                        set_workspace_view.set(WorkspaceView::Discovery);
                        set_draft_to_restore.set(None);
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
                        set_prompt_checkpoint.set(String::new());
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
                                                (
                                                    answer.clarification_id.clone(),
                                                    answer.answer.clone(),
                                                )
                                            })
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                set_clarification_answers.set(answers);
                                let prompt_value = workflow
                                    .as_ref()
                                    .map(|state| state.requirement_text.clone())
                                    .unwrap_or_default();
                                set_prompt_checkpoint.set(prompt_value.clone());
                                set_prompt.set(prompt_value);
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
        }
    });

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

    let switch_app_state = app_state.clone();
    let select_project = move |value: String| {
        let Ok(project_id) = value.parse::<i64>() else {
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
        if current_project
            .get()
            .is_some_and(|current| current.id == project.id)
            || project_busy.get()
        {
            return;
        }
        let generation_running = status.get() == RunStatus::Running
            || controller.get().is_some()
            || workflow_busy.get()
            || test_case_busy.get()
            || test_case_running.get()
            || test_running.get()
            || analysis_running.get()
            || validation_running.get()
            || version_saving.get()
            || verification_busy.get()
            || rollback_busy.get()
            || member_busy.get()
            || model_library_busy.get()
            || service_key_busy.get()
            || service_busy.get()
            || outcome_busy.get()
            || proposal_busy.get()
            || review_loading.get();
        if generation_running {
            set_project_error.set(Some(
                "当前项目仍有任务运行中，请等待任务完成后再切换项目。".into(),
            ));
            return;
        }
        if has_unsaved_changes.get() {
            let message = if local_draft.get_untracked().is_some() {
                "当前项目有本地未保存内容。确认后会先保存为本地草稿，切换后可从项目中恢复。取消可继续编辑。"
            } else {
                "当前项目有未确认的需求方案。已生成的需求分析会保留在服务端，确定切换吗？取消可继续编辑。"
            };
            let confirmed = web_sys::window()
                .is_some_and(|window| window.confirm_with_message(message).unwrap_or(false));
            if !confirmed {
                return;
            }
            if let Some(snapshot) = local_draft.get_untracked()
                && !switch_app_state.save_workspace_draft(&snapshot)
            {
                set_project_error.set(Some(
                    "本地草稿保存失败，未执行项目切换；请检查浏览器存储空间后重试。".into(),
                ));
                return;
            }
        }
        set_project_busy.set(true);
        set_project_error.set(None);
        set_draft_to_restore.set(None);
        let run_switch_app_state = switch_app_state.clone();
        spawn_local(async move {
            match api::get_project_workspace(&token_value, project.id).await {
                Ok(workspace) => {
                    let draft = load_compatible_workspace_draft(
                        &run_switch_app_state,
                        project.id,
                        workspace.version.as_ref().map(|version| version.id),
                    );
                    let workflow = match api::get_decision_workflow(&token_value, project.id).await
                    {
                        Ok(workflow) => workflow,
                        Err(message) => {
                            set_project_error.set(Some(message));
                            set_project_busy.set(false);
                            return;
                        }
                    };
                    set_workspace_view.set(load_workspace_view(project.id));
                    set_workflow_error.set(None);
                    set_pending_review.set(None);
                    set_pending_candidate_id.set(None);
                    set_pending_knowledge_citations.set(Vec::new());
                    set_review_confirmed.set(false);
                    set_versions.set(Vec::new());
                    set_selected_version_id.set(None);
                    set_selected_version_review.set(None);
                    set_audit_events.set(Vec::new());
                    set_rollback_reason.set(String::new());
                    set_rollback_confirmed.set(false);
                    set_verification_report.set(None);
                    set_publish_confirmed.set(false);
                    set_release_error.set(None);
                    set_status.set(RunStatus::Idle);
                    set_assistant_delta.set(String::new());
                    set_agent_activity.set("等待新的建模任务".into());
                    set_show_test.set(false);
                    set_test_xml.set(String::new());
                    set_test_result.set(None);
                    set_test_error.set(None);
                    set_validation_result.set(None);
                    set_validation_error.set(None);
                    follow_messages.set(true);
                    unread_messages.set(false);
                    let has_version = workspace.version.is_some();
                    let version = workspace.version.clone();
                    switch_app_state.set_active_project(Some(project.id));
                    set_draft_to_restore.set(draft);
                    set_current_project.set(Some(workspace.project));
                    set_current_version.set(version.clone());
                    set_dsl.set(workspace.dsl_yaml.clone());
                    set_source_dsl.set(workspace.dsl_yaml.unwrap_or_default());
                    set_source_dsl_checkpoint.set(source_dsl.get_untracked());
                    set_xml.set(workspace.dmn_xml);
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
                    {
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
                        let prompt_value = workflow
                            .as_ref()
                            .map(|state| state.requirement_text.clone())
                            .unwrap_or_default();
                        set_prompt_checkpoint.set(prompt_value.clone());
                        set_prompt.set(prompt_value);
                        // 项目进入后统一从需求与方案开始；生成模型由确认方案入口触发。
                        set_workspace_view.set(WorkspaceView::Discovery);
                        set_decision_workflow.set(workflow);
                    }
                }
                Err(message) => set_project_error.set(Some(message)),
            }
            set_project_busy.set(false);
        });
    };

    let restore_workspace_draft = move |_| {
        let Some(snapshot) = draft_to_restore.get_untracked() else {
            return;
        };
        let restored_dsl = snapshot.dsl.clone();
        let restored_xml = snapshot.xml.clone();
        set_prompt.set(snapshot.prompt.clone());
        set_clarification_answers.set(snapshot.clarification_answers.clone());
        set_pending_review.set(snapshot.pending_review.clone());
        set_pending_candidate_id.set(snapshot.pending_candidate_id);
        set_pending_knowledge_citations.set(snapshot.pending_knowledge_citations.clone());
        set_review_confirmed.set(snapshot.review_confirmed);
        set_dsl.set(restored_dsl.clone());
        set_source_dsl.set(if snapshot.source_dsl.is_empty() {
            restored_dsl.clone().unwrap_or_default()
        } else {
            snapshot.source_dsl.clone()
        });
        set_xml.set(restored_xml.clone());
        set_test_xml.set(restored_xml.unwrap_or_default());
        set_test_result.set(None);
        set_test_error.set(None);
        set_validation_result.set(None);
        set_validation_error.set(None);
        set_generation_mode.set(if snapshot.modify_mode {
            GenerationMode::Modify
        } else {
            GenerationMode::Create
        });
        set_workspace_view.set(workspace_view_from_key(&snapshot.workspace_view));
        set_draft_to_restore.set(None);
        if snapshot.version_mismatch {
            set_version_save_status.set(Some(
                "服务端版本已变化，仅恢复需求和澄清答案；旧模型变更未恢复，请重新生成".into(),
            ));
            set_agent_activity.set("版本基线已变化，请重新生成模型".into());
        } else {
            set_version_save_status.set(Some("已恢复本地草稿，请继续确认或保存".into()));
            set_agent_activity.set("本地草稿已恢复".into());
        }
    };

    let discard_workspace_draft = Callback::new({
        let discard_app_state = app_state.clone();
        move |_: leptos::ev::MouseEvent| {
            if let Some(project) = current_project.get_untracked() {
                discard_app_state.clear_workspace_draft(project.id);
            }
            set_draft_to_restore.set(None);
            set_version_save_status.set(Some("已使用服务端内容，本地草稿已移除".into()));
        }
    });

    let select_version_review = move |raw: String| {
        if raw.is_empty() {
            set_selected_version_id.set(None);
            set_selected_version_review.set(None);
            set_behavior_comparison.set(None);
            set_regression_report.set(None);
            set_regression_loading.set(false);
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
        set_behavior_comparison.set(None);
        set_regression_report.set(None);
        set_regression_loading.set(false);
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
                    if versions.get_untracked().iter().any(|version| {
                        version.id == version_id && version.base_version_id.is_some()
                    }) {
                        if let Ok(comparison) = api::get_version_behavior_comparison(
                            &token_value,
                            project.id,
                            version_id,
                        )
                        .await
                        {
                            if is_current_request() {
                                set_behavior_comparison.set(Some(comparison));
                            }
                        }
                    }
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

    let run_selected_regression = move || {
        let (Some(project), Some(version_id), Some(token_value)) = (
            current_project.get(),
            selected_version_id.get(),
            token.get(),
        ) else {
            return;
        };
        set_regression_loading.set(true);
        set_project_error.set(None);
        spawn_local(async move {
            let result =
                api::run_version_regression_tests(&token_value, project.id, version_id).await;
            if selected_version_id.get_untracked() != Some(version_id)
                || current_project
                    .get_untracked()
                    .is_none_or(|current| current.id != project.id)
            {
                return;
            }
            match result {
                Ok(report) => set_regression_report.set(Some(report)),
                Err(message) => set_project_error.set(Some(message)),
            }
            set_regression_loading.set(false);
        });
    };

    let create_rollback_draft = move || {
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
                    set_source_dsl_checkpoint.set(source_dsl.get_untracked());
                    set_xml.set(workspace.dmn_xml);
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
        if !can_model.get() {
            return;
        }
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
        if !can_model.get() {
            return;
        }
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
                    // Invalidate every list request started before the
                    // generation transaction completed. The follow-up list
                    // request is allowed to refresh persisted cases, but an
                    // older empty response must never replace this result.
                    set_test_case_load_revision.update(|revision| *revision += 1);
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
        if !can_model.get() {
            return;
        }
        let (Some(project), Some(version), Some(token_value)) =
            (current_project.get(), current_version.get(), token.get())
        else {
            return;
        };
        if test_case_busy.get() || matches!(version.status.as_str(), "published" | "retired") {
            return;
        }
        let case_name = decision_test_cases
            .get()
            .into_iter()
            .find(|test_case| test_case.id == case_id)
            .map(|test_case| test_case.name)
            .unwrap_or_else(|| "该验收案例".into());
        let confirmed = web_sys::window().is_some_and(|window| {
            window
                .confirm_with_message(&format!("确定删除“{case_name}”吗？此操作无法撤销。"))
                .unwrap_or(false)
        });
        if !confirmed {
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
                    set_selected_test_case_ids.update(|selected| {
                        selected.remove(&case_id);
                    });
                    set_verification_report.set(None);
                    set_publish_confirmed.set(false);
                    mark_version_draft(version.id);
                }
                Err(message) => set_test_case_error.set(Some(message)),
            }
            set_test_case_busy.set(false);
        });
    };

    let save_member = move || {
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

    let select_published_service = move |service_id: String| {
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
            let copied = copy_text_to_clipboard(&secret).await;
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
            let copied = copy_text_to_clipboard(&command).await;
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

    let copy_service_endpoint = move |(service_id, endpoint): (String, String)| {
        spawn_local(async move {
            let copied = copy_text_to_clipboard(&endpoint).await;
            if copied {
                set_service_endpoint_copied.set(Some(service_id));
            }
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

    let route_copilot_write = Callback::new(move |intent: String| {
        if !can_model.get() || workflow_locked.get() {
            return;
        }
        set_prompt.set(intent);
        set_generation_mode.set(if current_version.get().is_some() {
            GenerationMode::Modify
        } else {
            GenerationMode::Create
        });
        set_workspace_view.set(WorkspaceView::Discovery);
        set_mobile_model_focus.set(MobileModelFocus::Conversation);
        set_workflow_error.set(None);
        set_pending_requirement_review.set(None);
        set_selected_review_model.set(None);
    });

    let analyze_requirement_action = move |retry_of_orchestration_id: Option<i64>| {
        if !can_model.get() || workflow_locked.get() {
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
        let controller = AbortController::new().expect("AbortController should be available");
        set_workflow_busy.set(true);
        set_workflow_analyzing.set(true);
        set_workflow_controller.set(Some(controller.clone()));
        set_workflow_orchestration_stage.set(0);
        set_workflow_orchestration_events.set(Vec::new());
        set_workflow_error.set(None);
        set_pending_requirement_review.set(None);
        set_selected_review_model.set(None);
        set_workflow_retry_id.set(None);
        set_workflow_orchestration_id.set(None);
        set_workspace_view.set(WorkspaceView::Discovery);
        set_agent_activity.set("正在分析业务目标、输入输出和关键歧义".into());
        scroll_to_workflow_anchor("workflow-analysis-progress");
        let set_decision_workflow = set_decision_workflow;
        let set_clarification_answers = set_clarification_answers;
        let set_workflow_error = set_workflow_error;
        let set_agent_activity = set_agent_activity;
        let set_workflow_busy = set_workflow_busy;
        let set_workflow_analyzing = set_workflow_analyzing;
        let set_workflow_orchestration_stage = set_workflow_orchestration_stage;
        let set_workflow_orchestration_events = set_workflow_orchestration_events;
        let set_workflow_orchestration_id = set_workflow_orchestration_id;
        spawn_local(async move {
            let mut analysis_failed = false;
            let result = api::analyze_requirement_stream(
                &token_value,
                project.id,
                text,
                retry_of_orchestration_id,
                controller.clone(),
                |event| match event {
                    SseEvent::Orchestration(id) => {
                        set_workflow_orchestration_id.set(Some(id));
                        set_workflow_retry_id.set(Some(retry_of_orchestration_id.unwrap_or(id)))
                    }
                    SseEvent::Step(message) => {
                        if message.starts_with("编排阶段：") {
                            let stage = if message.contains("需求分析智能体") {
                                1
                            } else if message.contains("业务模式识别器") {
                                2
                            } else if message.contains("澄清排序器") {
                                3
                            } else if message.contains("校验并保存") {
                                4
                            } else {
                                0
                            };
                            set_workflow_orchestration_stage.set(stage);
                        }
                        set_agent_activity.set(agent_activity_for_step(&message));
                    }
                    SseEvent::WorkflowResult(workflow) => {
                        set_workflow_retry_id.set(None);
                        set_clarification_answers.set(BTreeMap::new());
                        set_prompt_checkpoint.set(workflow.requirement_text.clone());
                        set_decision_workflow.set(Some(workflow));
                        set_agent_activity.set("需求分析完成，请核对摘要和澄清问题".into());
                        scroll_to_workflow_anchor("workflow-clarifications");
                    }
                    SseEvent::Error(message) => {
                        analysis_failed = true;
                        set_workflow_retry_id.set(None);
                        set_workflow_error.set(Some(message));
                        set_agent_activity.set("需求分析未完成".into());
                    }
                    SseEvent::RetryableFailure { message, retryable } => {
                        analysis_failed = true;
                        if !retryable {
                            set_workflow_retry_id.set(None);
                        }
                        set_workflow_error.set(Some(message));
                        set_agent_activity.set("需求分析未完成".into());
                    }
                    event => {
                        if let Some((message, _)) = orchestration_event_update(&event) {
                            set_agent_activity.set(message.clone());
                            set_workflow_orchestration_events.update(|events| {
                                append_orchestration_progress(events, message);
                            });
                        }
                    }
                },
            )
            .await;
            if !controller.signal().aborted() {
                if let Err(message) = result {
                    analysis_failed = true;
                    set_workflow_retry_id.set(None);
                    set_workflow_error.set(Some(message));
                    set_agent_activity.set("需求分析未完成".into());
                }
                if analysis_failed {
                    match api::get_pending_requirement_review(&token_value, project.id).await {
                        Ok(Some(review)) => {
                            set_workspace_view.set(WorkspaceView::Discovery);
                            set_selected_review_model.set(None);
                            set_pending_requirement_review.set(Some(review));
                        }
                        Ok(None) => {
                            set_workflow_error.update(|error| {
                                if let Some(error) = error
                                    && (error.contains("多数共识") || error.contains("人工复核"))
                                {
                                    error.push_str("；未读取到可展示的模型候选，请刷新后重试分析");
                                }
                            });
                        }
                        Err(message) => {
                            set_workflow_error.update(|error| {
                                if let Some(error) = error {
                                    error.push_str(&format!("；读取候选结果失败：{message}"));
                                }
                            });
                        }
                    }
                }
            }
            set_workflow_orchestration_id.set(None);
            set_workflow_controller.set(None);
            set_workflow_busy.set(false);
            set_workflow_analyzing.set(false);
        });
    };

    let analyze_requirement = move |_| analyze_requirement_action(None);
    let import_requirement_document = move |event: web_sys::Event| {
        let input = event_target::<web_sys::HtmlInputElement>(&event);
        let Some(file) = input.files().and_then(|files| files.item(0)) else {
            return;
        };
        input.set_value("");
        let filename = file.name();
        let supported = filename
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !matches!(supported.as_str(), "txt" | "md" | "markdown") {
            set_requirement_import_error.set(Some("仅支持 TXT 和 Markdown 文件".into()));
            return;
        }
        if file.size() > 20_000.0 {
            set_requirement_import_error.set(Some("需求文档不能超过 20,000 字节".into()));
            return;
        }
        if workflow_locked.get_untracked() || requirement_importing.get_untracked() {
            return;
        }
        set_requirement_importing.set(true);
        set_requirement_import_error.set(None);
        spawn_local(async move {
            let text = JsFuture::from(file.text())
                .await
                .ok()
                .and_then(|value| value.as_string());
            match text {
                Some(text) if !text.trim().is_empty() && text.len() <= 20_000 => {
                    set_prompt.set(text);
                }
                Some(_) => set_requirement_import_error
                    .set(Some("文档正文不能为空且不能超过 20,000 字节".into())),
                None => set_requirement_import_error
                    .set(Some("读取文档失败，请确认文件是有效文本".into())),
            }
            set_requirement_importing.set(false);
        });
    };
    let retry_requirement_analysis = move |_| {
        if let Some(orchestration_id) = workflow_retry_id.get() {
            analyze_requirement_action(Some(orchestration_id));
        }
    };
    let confirm_human_requirement_review = move |_| {
        let (Some(project), Some(token_value), Some(review), Some(model)) = (
            current_project.get(),
            token.get(),
            pending_requirement_review.get(),
            selected_review_model.get(),
        ) else {
            return;
        };
        if review_busy.get() {
            return;
        }
        set_review_busy.set(true);
        spawn_local(async move {
            match api::confirm_requirement_review(&token_value, project.id, review.run_id, model)
                .await
            {
                Ok(workflow) => {
                    set_prompt_checkpoint.set(workflow.requirement_text.clone());
                    set_prompt.set(workflow.requirement_text.clone());
                    set_decision_workflow.set(Some(workflow));
                    set_pending_requirement_review.set(None);
                    set_selected_review_model.set(None);
                    set_workflow_error.set(None);
                    set_agent_activity.set("人工复核完成，请继续核对需求摘要与澄清问题".into());
                }
                Err(message) => set_workflow_error.set(Some(message)),
            }
            set_review_busy.set(false);
        });
    };
    let stop_workflow_operation = move |_| {
        if let Some(active) = workflow_controller.get() {
            let was_planning = workflow_planning.get();
            let was_facilitating = workflow_facilitating.get();
            if let (Some(project), Some(token_value), Some(orchestration_id)) = (
                current_project.get(),
                token.get(),
                workflow_orchestration_id.get(),
            ) {
                let set_agent_activity = set_agent_activity;
                spawn_local(async move {
                    let cancel_result =
                        api::cancel_agent_orchestration(&token_value, project.id, orchestration_id)
                            .await;
                    active.abort();
                    if let Err(message) = cancel_result {
                        set_agent_activity
                            .set(format!("本地任务已停止，服务端取消未确认：{message}"));
                    }
                });
            } else {
                active.abort();
            }
            set_workflow_retry_id.set(None);
            set_plan_retry_id.set(None);
            set_facilitator_retry_id.set(None);
            set_workflow_orchestration_id.set(None);
            set_agent_activity.set(if was_planning {
                "已停止方案规划".into()
            } else if was_facilitating {
                "已停止澄清主持".into()
            } else {
                "已停止需求分析".into()
            });
        }
    };

    let create_decision_plan_action = move |retry_of_orchestration_id: Option<i64>| {
        if !can_model.get() || workflow_locked.get() {
            return;
        }
        if !workflow_current.get() {
            set_workflow_error.set(Some("需求或模型基线已变化，请先重新分析".into()));
            return;
        }
        if missing_clarifications.get() > 0 {
            set_workflow_error.set(Some(format!(
                "还有 {} 个必答问题，请先补充回答",
                missing_clarifications.get()
            )));
            return;
        }
        let Some(workflow) = decision_workflow.get() else {
            return;
        };
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
        let controller = AbortController::new().expect("AbortController should be available");
        set_workflow_busy.set(true);
        set_workflow_planning.set(true);
        set_workflow_controller.set(Some(controller.clone()));
        set_workflow_orchestration_stage.set(0);
        set_workflow_orchestration_events.set(Vec::new());
        set_workflow_orchestration_id.set(None);
        set_workflow_error.set(None);
        set_plan_retry_id.set(None);
        set_agent_activity.set("正在规划决策、依赖和验收标准".into());
        let set_workflow_orchestration_id = set_workflow_orchestration_id;
        let set_workflow_orchestration_stage = set_workflow_orchestration_stage;
        let set_workflow_orchestration_events = set_workflow_orchestration_events;
        let set_plan_retry_id = set_plan_retry_id;
        let set_workflow_error = set_workflow_error;
        let set_agent_activity = set_agent_activity;
        let set_prompt_checkpoint = set_prompt_checkpoint;
        let set_decision_workflow = set_decision_workflow;
        let set_workflow_controller = set_workflow_controller;
        let set_workflow_planning = set_workflow_planning;
        let set_workflow_busy = set_workflow_busy;
        spawn_local(async move {
            let result = api::create_decision_plan_stream(
                &token_value,
                project.id,
                answers,
                workflow.updated_at,
                retry_of_orchestration_id,
                controller.clone(),
                |event| match event {
                    SseEvent::Orchestration(id) => {
                        set_workflow_orchestration_id.set(Some(id));
                        set_plan_retry_id.set(Some(retry_of_orchestration_id.unwrap_or(id)));
                    }
                    SseEvent::Step(message) => {
                        set_workflow_orchestration_stage.set(1);
                        set_agent_activity.set(agent_activity_for_step(&message));
                    }
                    SseEvent::WorkflowResult(workflow) => {
                        set_plan_retry_id.set(None);
                        let answer_values = workflow
                            .answers
                            .iter()
                            .map(|answer| (answer.clarification_id.clone(), answer.answer.clone()))
                            .collect();
                        set_clarification_answers.set(answer_values);
                        set_prompt_checkpoint.set(workflow.requirement_text.clone());
                        set_decision_workflow.set(Some(workflow));
                        set_agent_activity.set("决策方案已生成，等待确认".into());
                    }
                    SseEvent::RetryableFailure { message, retryable } => {
                        if !retryable {
                            set_plan_retry_id.set(None);
                        }
                        set_workflow_error.set(Some(message));
                        set_agent_activity.set("决策方案规划未完成".into());
                    }
                    SseEvent::Error(message) => {
                        set_plan_retry_id.set(None);
                        set_workflow_error.set(Some(message));
                        set_agent_activity.set("决策方案规划未完成".into());
                    }
                    event => {
                        if let Some((message, _)) = orchestration_event_update(&event) {
                            set_agent_activity.set(message.clone());
                            set_workflow_orchestration_events.update(|events| {
                                append_orchestration_progress(events, message);
                            });
                        }
                    }
                },
            )
            .await;
            if !controller.signal().aborted() {
                if let Err(message) = result {
                    set_plan_retry_id.set(None);
                    set_workflow_error.set(Some(message));
                    set_agent_activity.set("决策方案规划未完成".into());
                }
            }
            set_workflow_orchestration_id.set(None);
            set_workflow_controller.set(None);
            set_workflow_planning.set(false);
            set_workflow_busy.set(false);
        });
    };

    let continue_clarification_action = move |retry_of_orchestration_id: Option<i64>| {
        if !can_model.get() || workflow_locked.get() || !workflow_current.get() {
            return;
        }
        if missing_clarifications.get() > 0 {
            set_workflow_error.set(Some(format!(
                "还有 {} 个必答问题，请先回答或修改答案",
                missing_clarifications.get()
            )));
            return;
        }
        let Some(workflow) = decision_workflow.get() else {
            return;
        };
        let Some(project) = current_project.get() else {
            return;
        };
        let Some(token_value) = token.get() else {
            return;
        };
        let current_round = workflow
            .clarifications
            .iter()
            .map(|item| item.round)
            .max()
            .unwrap_or(1);
        let answers = clarification_answers.get();
        let answers = workflow
            .clarifications
            .iter()
            .filter(|item| item.round == current_round)
            .filter_map(|item| {
                answers.get(&item.id).map(|answer| ClarificationAnswer {
                    clarification_id: item.id.clone(),
                    answer: answer.clone(),
                })
            })
            .collect::<Vec<_>>();
        set_workflow_busy.set(true);
        set_workflow_facilitating.set(true);
        let controller = AbortController::new().expect("AbortController should be available");
        set_workflow_controller.set(Some(controller.clone()));
        set_workflow_orchestration_id.set(None);
        set_workflow_orchestration_stage.set(0);
        set_workflow_orchestration_events.set(Vec::new());
        set_workflow_error.set(None);
        set_pending_requirement_review.set(None);
        set_selected_review_model.set(None);
        set_facilitator_retry_id.set(None);
        set_agent_activity.set("澄清主持人正在根据本轮回答检查未决歧义".into());
        let set_workflow_busy = set_workflow_busy;
        let set_workflow_facilitating = set_workflow_facilitating;
        let set_workflow_error = set_workflow_error;
        let set_workflow = set_decision_workflow;
        let set_answers = set_clarification_answers;
        let set_activity = set_agent_activity;
        let set_orchestration_id = set_workflow_orchestration_id;
        let set_orchestration_stage = set_workflow_orchestration_stage;
        let set_orchestration_events = set_workflow_orchestration_events;
        let set_controller = set_workflow_controller;
        let set_facilitator_retry_id = set_facilitator_retry_id;
        let set_pending_requirement_review = set_pending_requirement_review;
        let set_selected_review_model = set_selected_review_model;
        let set_workspace_view = set_workspace_view;
        let mut facilitator_failed = false;
        spawn_local(async move {
            let result = api::continue_workflow_clarifications(
                &token_value,
                project.id,
                answers,
                workflow.updated_at,
                retry_of_orchestration_id,
                controller.clone(),
                |event| match event {
                    SseEvent::Orchestration(id) => {
                        set_orchestration_id.set(Some(id));
                        set_facilitator_retry_id.set(Some(retry_of_orchestration_id.unwrap_or(id)));
                    }
                    SseEvent::Step(message) => {
                        set_orchestration_stage.set(1);
                        set_activity.set(agent_activity_for_step(&message));
                    }
                    SseEvent::WorkflowResult(updated) => {
                        set_facilitator_retry_id.set(None);
                        set_pending_requirement_review.set(None);
                        set_selected_review_model.set(None);
                        let answer_values = updated
                            .answers
                            .iter()
                            .map(|answer| (answer.clarification_id.clone(), answer.answer.clone()))
                            .collect();
                        let converged = updated.status == "analyzed";
                        set_answers.set(answer_values);
                        set_workflow.set(Some(updated));
                        set_activity.set(if converged {
                            "澄清已收敛，可以生成决策方案".into()
                        } else {
                            "本轮澄清完成，请回答新增问题".into()
                        });
                    }
                    SseEvent::Error(message) => {
                        facilitator_failed = true;
                        set_facilitator_retry_id.set(None);
                        set_workflow_error.set(Some(message));
                        set_activity.set("澄清主持未完成".into());
                    }
                    SseEvent::RetryableFailure { message, retryable } => {
                        facilitator_failed = true;
                        if !retryable {
                            set_facilitator_retry_id.set(None);
                        }
                        set_workflow_error.set(Some(message));
                        set_activity.set("澄清主持未完成".into());
                    }
                    event => {
                        if let Some((message, _)) = orchestration_event_update(&event) {
                            set_activity.set(message.clone());
                            set_orchestration_events.update(|events| {
                                append_orchestration_progress(events, message);
                            });
                        }
                    }
                },
            )
            .await;
            if !controller.signal().aborted() {
                if let Err(message) = result {
                    facilitator_failed = true;
                    set_facilitator_retry_id.set(None);
                    set_workflow_error.set(Some(message));
                    set_activity.set("澄清主持未完成".into());
                }
                if facilitator_failed {
                    match api::get_pending_requirement_review(&token_value, project.id).await {
                        Ok(Some(review)) => {
                            set_workspace_view.set(WorkspaceView::Discovery);
                            set_selected_review_model.set(None);
                            let activity = if review.purpose == "facilitator" {
                                "澄清主持结果未达成多数共识，请选择候选并保留本轮回答"
                            } else {
                                "多模型未达成共识，请选择候选结果进行人工复核"
                            };
                            set_pending_requirement_review.set(Some(review));
                            set_activity.set(activity.into());
                        }
                        Ok(None) => {
                            set_workflow_error.update(|error| {
                                if let Some(error) = error
                                    && error.contains("未达成多数共识")
                                {
                                    error.push_str("；未读取到可展示的模型候选，请刷新后重试");
                                }
                            });
                        }
                        Err(message) => {
                            set_workflow_error.update(|error| {
                                if let Some(error) = error {
                                    error.push_str(&format!("；读取人工复核候选失败：{message}"));
                                }
                            });
                        }
                    }
                }
            }
            set_orchestration_id.set(None);
            set_controller.set(None);
            set_workflow_facilitating.set(false);
            set_workflow_busy.set(false);
        });
    };

    let create_decision_plan = move |_| create_decision_plan_action(None);
    let continue_clarification = move |_| continue_clarification_action(None);
    let retry_clarification = move |_| {
        if let Some(orchestration_id) = facilitator_retry_id.get() {
            continue_clarification_action(Some(orchestration_id));
        }
    };
    let retry_decision_plan = move |_| {
        if let Some(orchestration_id) = plan_retry_id.get() {
            create_decision_plan_action(Some(orchestration_id));
        }
    };

    let confirm_decision_plan_action = move || {
        if !can_model.get() || workflow_locked.get() {
            return;
        }
        if !workflow_current.get() || workflow_answers_changed.get() {
            set_workflow_error.set(Some(
                "需求或回答已变化，请重新分析或生成方案后再确认".into(),
            ));
            return;
        }
        let Some(workflow) = decision_workflow.get() else {
            return;
        };
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
        set_workflow_confirming.set(true);
        set_workflow_error.set(None);
        spawn_local(async move {
            match api::confirm_decision_plan(&token_value, project.id, workflow.updated_at).await {
                Ok(workflow) => {
                    set_prompt.set(workflow.requirement_text.clone());
                    set_prompt_checkpoint.set(workflow.requirement_text.clone());
                    set_decision_workflow.set(Some(workflow));
                    set_agent_activity.set("方案已确认，可以生成可执行模型".into());
                }
                Err(message) => set_workflow_error.set(Some(message)),
            }
            set_workflow_confirming.set(false);
            set_workflow_busy.set(false);
        });
    };

    let confirm_decision_plan = move |_| confirm_decision_plan_action();

    let generate_model_action = move || {
        if !can_model.get() {
            return;
        }
        let workflow = decision_workflow.get();
        let confirmed = workflow
            .as_ref()
            .is_some_and(|workflow| workflow.status == "confirmed" && workflow.plan.is_some());
        let simple_direct = workflow
            .as_ref()
            .is_some_and(simple_direct_workflow_eligible);
        if !(confirmed || simple_direct)
            || workflow_locked.get()
            || !workflow_current.get()
            || workflow_answers_changed.get()
        {
            return;
        }
        let has_existing_version = current_version.get().is_some();
        if has_existing_version && source_dsl.get().trim().is_empty() {
            set_error.set(Some(
                "当前版本的 DSL 基线尚未加载，无法生成受控修改；请刷新项目后重试".into(),
            ));
            return;
        }
        set_generation_mode.set(if has_existing_version {
            GenerationMode::Modify
        } else {
            GenerationMode::Create
        });
        set_workspace_view.set(WorkspaceView::Model);
        submit();
    };

    let generate_model_from_plan = move |_| generate_model_action();

    let on_keydown = move |event: KeyboardEvent| {
        if event.is_composing()
            || project_busy.get()
            || !can_model.get()
            || !(event.ctrl_key() || event.meta_key())
            || event.key() != "Enter"
        {
            return;
        }
        event.prevent_default();

        if workflow_locked.get() {
            return;
        }
        if !workflow_current.get() {
            analyze_requirement_action(None);
            return;
        }
        if workflow_answers_changed.get() {
            create_decision_plan_action(None);
            return;
        }

        match decision_workflow
            .get()
            .as_ref()
            .map(|workflow| workflow.status.as_str())
        {
            None => analyze_requirement_action(None),
            Some("analyzed" | "clarification") => create_decision_plan_action(None),
            Some("planned") => confirm_decision_plan_action(),
            Some("confirmed") => generate_model_action(),
            Some(_) => analyze_requirement_action(None),
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

    let flight_delay_resolution = include_str!("./demo/FlightDelayResolution.md");
    let loan_approval_process = include_str!("./demo/LoanApprovalProcess.md");
    let order_discount_calculation = include_str!("./demo/OrderDiscountCalculation.md");

    view! {
        <main class=move || format!("app-shell workbench-shell page-transition {}", theme.get().class()) inert=move || project_busy.get() aria-busy=move || project_busy.get()>
            <Header
                status=status
                user_id=user_id
                user_role=user_role
                validation_running=validation_running
                xml=xml
                has_unsaved_changes=has_unsaved_changes
                on_validate=Callback::new(move |_| validate_model(()))
            />

            <ProjectBar
                projects=projects
                versions=versions
                selected_version_id=selected_version_id
                project_busy=project_busy
                version_saving=version_saving
                review_loading=review_loading
                version_save_status=version_save_status
                project_error=project_error
                workspace_draft_storage_error=app_state.ui.workspace_draft_storage_error.read_only()
                on_select_project=Callback::new(move |value: String| select_project(value))
                on_select_version_review=Callback::new(move |value: String| select_version_review(value))
            />

            <Show when=move || draft_to_restore.get().is_some()>
                <section class="workspace-draft-recovery" aria-label="本地草稿恢复">
                    <div class="workspace-draft-recovery-copy">
                        <strong>"发现本地草稿"</strong>
                        <span>"这里保存了离开工作台前未确认的需求或模型变更。请选择恢复，或使用服务端内容继续。"</span>
                    </div>
                    <div class="workspace-draft-recovery-actions">
                        <button class="button button-primary" type="button" on:click=restore_workspace_draft>
                            "恢复本地草稿"
                        </button>
                        <button class="button button-quiet" type="button" on:click=move |event| discard_workspace_draft.run(event)>
                            "使用服务端内容"
                        </button>
                    </div>
                </section>
            </Show>

            <WorkspaceNavigation
                active=workspace_view
                model_available=Signal::derive(move || current_version.get().is_some() || dsl.get().is_some())
                version_count=Signal::derive(move || versions.get().len())
                test_count=Signal::derive(move || decision_test_cases.get().len())
                service_count=Signal::derive(move || published_services.get().len())
                finding_count=Signal::derive(move || decision_analytics.get().map(|report| report.findings.len()).unwrap_or_default())
                member_count=Signal::derive(move || project_members.get().len())
                on_select=Callback::new(move |view| {
                    if view == WorkspaceView::Discovery {
                        set_generation_mode.set(GenerationMode::Create);
                    } else if view == WorkspaceView::Model {
                        set_generation_mode.set(GenerationMode::Modify);
                    }
                    set_workspace_view.set(view);
                })
            />
            <WorkspaceOverview
                active=workspace_view
                test_count=Signal::derive(move || decision_test_cases.get().len())
                version_count=Signal::derive(move || versions.get().len())
                service_count=Signal::derive(move || published_services.get().len())
                has_unsaved_changes=has_unsaved_changes
            />

            <Show when=move || workspace_view.get() == WorkspaceView::Collaboration && current_project.get().is_some()>
                <section id="workspace-panel-collaboration" class="collaboration-panel" role="tabpanel" aria-labelledby="workspace-tab-collaboration" tabindex="0" inert=move || project_busy.get()>
                    <div class="collaboration-heading">
                        <div>
                            <span class="yaml-kicker">"PROJECT COLLABORATION"</span>
                            <strong>"成员与发布职责"</strong>
                            <small>"负责人负责成员配置和建模；建模、复核、审批由不同角色完成，同一账号可添加多个角色。"</small>
                        </div>
                        /*<Show when=move || current_version.get().is_some_and(|version| version.reviewed_by.is_none())>
                            <button class="button button-primary" on:click=review_current_version disabled=move || verification_busy.get()>
                                {move || if verification_busy.get() { "处理中..." } else { "独立复核当前版本" }}
                            </button>
                        </Show>*/
                    </div>
                    <div class="collaboration-layout">
                        <aside class="collaboration-overview" aria-label="协作上下文">
                            <div class="collaboration-context">
                                <span class="member-section-kicker">"CURRENT PROJECT"</span>
                                <strong>{move || current_project.get().map(|project| project.name).unwrap_or_else(|| "当前项目".into())}</strong>
                                <small>{move || current_version.get().map(|version| format!("当前版本 v{} · {}", version.version_number, version.status)).unwrap_or_else(|| "尚未生成版本".into())}</small>
                            </div>
                            <div class="member-role-guide" aria-label="项目职责说明">
                                <div class="member-role-guide-heading">
                                    <span class="member-section-kicker">"RESPONSIBILITY MODEL"</span>
                                    <strong>"职责边界"</strong>
                                    <small>"将建模、复核和发布职责分开，保持决策变更可追溯。"</small>
                                </div>
                                <div class="member-role-grid">
                                    <div class="member-role-card">
                                        <span class="member-role-icon" aria-hidden="true">"模"</span>
                                        <div><strong>"建模者"</strong><small>"需求、模型与案例"</small></div>
                                    </div>
                                    <div class="member-role-card">
                                        <span class="member-role-icon" aria-hidden="true">"核"</span>
                                        <div><strong>"复核者"</strong><small>"独立复核、批准案例、执行验收"</small></div>
                                    </div>
                                    <div class="member-role-card">
                                        <span class="member-role-icon" aria-hidden="true">"审"</span>
                                        <div><strong>"审批者"</strong><small>"门禁通过后发布服务"</small></div>
                                    </div>
                                    <div class="member-role-card">
                                        <span class="member-role-icon" aria-hidden="true">"阅"</span>
                                        <div><strong>"查看者"</strong><small>"只读访问项目内容"</small></div>
                                    </div>
                                </div>
                            </div>
                        </aside>
                        <MemberManagementPanel
                            members=project_members
                            member_username=member_username
                            set_member_username=set_member_username
                            member_role=member_role
                            set_member_role=set_member_role
                            member_busy=member_busy
                            member_error=member_error
                            member_status=member_status
                            on_save_member=Callback::new(move |_| save_member())
                            on_remove_member=Callback::new(move |(username, role)| remove_member(username, role))
                        />
                    </div>
                </section>
            </Show>

            <Show when=move || workspace_view.get() == WorkspaceView::Resources>
                <div id="workspace-panel-resources" class="resource-library-page" role="tabpanel" aria-labelledby="workspace-tab-resources" tabindex="0" inert=move || project_busy.get()>
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
                        <Show when=move || {
                            resource_view.get() == ResourceView::Models
                                && pattern_analytics.get().is_some()
                        }>
                            {move || pattern_analytics.get().map(|analytics| {
                                let has_top_patterns = !analytics.top_patterns.is_empty();
                                let top_patterns = analytics.top_patterns.clone();
                                view! {
                                <div class="pattern-library-analytics" aria-label="模式库统计">
                                    <span>{format!("模式 {} 个", analytics.total_patterns)}</span>
                                    <span>{format!("实际复用 {} 次", analytics.total_usage_count)}</span>
                                    <span>{format!("推荐 {} 次", analytics.total_recommendations)}</span>
                                    <span>{match analytics.acceptance_rate {
                                        Some(rate) => format!("反馈采纳率 {:.2}%", rate),
                                        None => "反馈采纳率暂无".to_string(),
                                    }}</span>
                                    <span>{match analytics.average_match_score {
                                        Some(score) => format!("平均匹配度 {:.1}", score),
                                        None => "平均匹配度暂无".to_string(),
                                    }}</span>
                                    <span>{match analytics.average_success_rate {
                                        Some(rate) => format!("平均成功率 {:.1}%", rate),
                                        None => "平均成功率暂无".to_string(),
                                    }}</span>
                                    <Show when=move || has_top_patterns>
                                        {view! {
                                            <div class="pattern-library-top" aria-label="高复用模式排行">
                                                <strong>"高复用模式"</strong>
                                                <div class="pattern-library-top-list">
                                                    {top_patterns.clone().into_iter().map(|pattern| view! {
                                                        <div class="pattern-library-top-item">
                                                            <div>
                                                                <strong>{pattern.name}</strong>
                                                                <span>{format!("复用 {} 次 · 演进 v{}", pattern.usage_count, pattern.evolution_number)}</span>
                                                            </div>
                                                            <small>{match pattern.success_rate {
                                                                Some(rate) => format!("反馈成功率 {:.1}%", rate),
                                                                None => "暂无反馈成功率".to_string(),
                                                            }}</small>
                                                            <small>{match pattern.last_used {
                                                                Some(last_used) => format!("最近复用 {}", last_used.format("%Y-%m-%d")),
                                                                None => "尚未实际复用".to_string(),
                                                            }}</small>
                                                        </div>
                                                    }).collect_view()}
                                                </div>
                                            </div>
                                        }}
                                    </Show>
                                </div>
                            }})}
                        </Show>
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
                            <button
                                class="button button-quiet"
                                on:click=request_pattern_recommendations
                                disabled=move || pattern_recommendations_busy.get() || model_library_query.get().trim().is_empty()
                            >{move || if pattern_recommendations_busy.get() { "检索中..." } else { "推荐相似模式" }}</button>
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
                            <Show when=move || !pattern_recommendations.get().is_empty()>
                                <section class="pattern-recommendations" aria-label="相似决策模式推荐">
                                    <strong>"相似已发布模式"</strong>
                                    <For
                                        each=move || pattern_recommendations.get()
                                        key=|item| item.pattern_id
                                        let:item
                                    >
                                        {
                                            let version_id = item.source_version_id;
                                            let score = item.match_score;
                                            let project_name = item.source_project_name.clone();
                                            let version_number = item.source_version_number;
                                            let description = item.description.clone();
                                            let pattern_type = item.pattern_type.clone();
                                            let recommendation_id = item.recommendation_id;
                                            view! {
                                                <article class="pattern-recommendation">
                                                    <div>
                                                        <strong>{item.name}</strong>
                                                        <span>{format!("{} · 匹配度 {:.0}%", pattern_type, score)}</span>
                                                    </div>
                                                    <small>{format!("来源：{} v{}", project_name, version_number)}</small>
                                                    <small>{format!("模式演进 v{}", item.evolution_number)}</small>
                                                    <p>{item.recommendation_reason}</p>
                                                    <small>{match item.success_rate {
                                                        Some(rate) => format!("历史反馈成功率 {:.0}% · 已复用 {} 次", rate, item.usage_count),
                                                        None => format!("已有 {} 次复用 · 暂无反馈统计", item.usage_count),
                                                    }}</small>
                                                    <small>{move || description.clone()}</small>
                                                    <div class="pattern-recommendation-feedback">
                                                        <button
                                                            class="button button-quiet"
                                                            on:click=move |_| submit_pattern_feedback(recommendation_id, true)
                                                            disabled=move || pattern_feedback_busy.get()
                                                        >"有帮助"</button>
                                                        <button
                                                            class="button button-quiet"
                                                            on:click=move |_| submit_pattern_feedback(recommendation_id, false)
                                                            disabled=move || pattern_feedback_busy.get()
                                                        >"不匹配"</button>
                                                    </div>
                                                    <button
                                                        class="button button-quiet"
                                                        on:click=move |_| {
                                                            if let Some(history) = model_library.get().into_iter().find(|entry| entry.source_version_id == Some(version_id)) {
                                                                select_model_library_item(history.source_kind, history.source_id, history.name);
                                                            }
                                                        }
                                                    >"选择并复用"</button>
                                                </article>
                                            }
                                        }
                                    </For>
                                </section>
                            </Show>
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
                                    <Show when=move || {
                                        is_history
                                            && (pattern_lineage_busy.get()
                                                || !pattern_lineage.get().is_empty())
                                    }>
                                        <section class="model-pattern-lineage" aria-label="模式演进链">
                                            <div class="model-pattern-lineage-heading">
                                                <span class="toolbar-label">"模式演进链"</span>
                                                <small>{move || if pattern_lineage_busy.get() { "读取中...".to_string() } else { format!("{} 个版本", pattern_lineage.get().len()) }}</small>
                                            </div>
                                            <div class="model-pattern-lineage-list">
                                                <For
                                                    each=move || pattern_lineage.get()
                                                    key=|lineage| lineage.pattern_id
                                                    let:lineage
                                                >
                                                    <div class="model-pattern-lineage-item">
                                                        <strong>{format!("v{} · {}", lineage.source_version_number, lineage.name)}</strong>
                                                        <small>{format!("演进序号 {} · {}", lineage.evolution_number, lineage.version_status)}</small>
                                                        <span>{lineage.description}</span>
                                                    </div>
                                                </For>
                                            </div>
                                        </section>
                                    </Show>
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
                                        on:click=move |event| instantiate_selected_model.run(event)
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
                <div id="workspace-panel-versions" role="tabpanel" aria-labelledby="workspace-tab-versions" tabindex="0" inert=move || project_busy.get()>
                <VersionGovernancePanel
                    versions=versions
                    current_version=current_version
                    audit_events=audit_events
                />
            <Show when=move || selected_version_review.get().is_some()>
                <VersionHistoryPanel
                    selected_version_review=selected_version_review
                    behavior_comparison=behavior_comparison
                    regression_report=regression_report
                    regression_loading=regression_loading
                    on_run_regression=Callback::new(move |_| run_selected_regression())
                    selected_version_id=selected_version_id
                    versions=versions
                    current_version=current_version
                    rollback_reason=rollback_reason
                    set_rollback_reason=set_rollback_reason
                    rollback_confirmed=rollback_confirmed
                    set_rollback_confirmed=set_rollback_confirmed
                    rollback_busy=rollback_busy
                    on_create_rollback=Callback::new(move |_| create_rollback_draft())
                    on_close=Callback::new(move |_| {
                        set_selected_version_id.set(None);
                        set_selected_version_review.set(None);
                        set_behavior_comparison.set(None);
                        set_regression_report.set(None);
                        set_regression_loading.set(false);
                    })
                />
            </Show>
                </div>
            </Show>

            <Show when=move || workspace_view.get() == WorkspaceView::Services && current_project.get().is_some_and(|project| project.published_version_id.is_some())>
                <section id="workspace-panel-services" class="published-services-panel" role="tabpanel" aria-labelledby="workspace-tab-services" tabindex="0" inert=move || project_busy.get()>
                    <div class="published-services-heading">
                        <div>
                            <span class="yaml-kicker">"PUBLISHED SERVICES"</span>
                            <strong>"已发布决策服务"</strong>
                            <small>{move || format!("{} 个可调用服务", published_services.get().len())}</small>
                        </div>
                        <span class="service-live-status">"LIVE"</span>
                    </div>
                    <ServiceCredentialsPanel
                        published_services=published_services
                        can_manage_service=can_manage_service
                        selected_service_id=selected_service_id
                        service_keys=service_keys
                        service_key_name=service_key_name
                        set_service_key_name=set_service_key_name
                        service_key_secret=service_key_secret
                        service_key_service_id=service_key_service_id
                        service_key_copy_status=service_key_copy_status
                        service_curl_copy_status=service_curl_copy_status
                        service_key_busy=service_key_busy
                        service_key_error=service_key_error
                        on_create_service_key=Callback::new(create_service_key)
                        on_copy_service_key=Callback::new(copy_service_key)
                        on_copy_service_curl=Callback::new(copy_service_curl)
                        on_revoke_service_key=Callback::new(revoke_service_key)
                    />
                    <Show
                        when=move || !published_services.get().is_empty()
                        fallback=move || view! { <div class="service-empty">"当前发布版本没有可调用决策服务"</div> }
                    >
                        <ServiceContractList
                            published_services=published_services
                            service_endpoint_copied=service_endpoint_copied
                            on_copy_endpoint=Callback::new(copy_service_endpoint)
                        />
                        <div class="service-runner">
                            <ServiceInvocationForm
                                published_services=published_services
                                selected_service_id=selected_service_id
                                service_business_reference=service_business_reference
                                set_service_business_reference=set_service_business_reference
                                service_input=service_input
                                set_service_input=set_service_input
                                service_busy=service_busy
                                on_select_service=Callback::new(select_published_service)
                                on_execute=Callback::new(execute_published_service)
                            />
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
                            <ServiceRunHistory
                                service_runs=service_runs
                                project_id=Signal::derive(move || current_project.get().map(|project| project.id))
                                token=Signal::derive(move || token.get())
                                reviewer_username=Signal::derive(move || user_id.get())
                                can_model=can_model
                                can_review=can_review
                                outcome_run_id=outcome_run_id
                                set_outcome_run_id=set_outcome_run_id
                                outcome_business_reference=outcome_business_reference
                                set_outcome_business_reference=set_outcome_business_reference
                                outcome_actual_json=outcome_actual_json
                                set_outcome_actual_json=set_outcome_actual_json
                                outcome_assessment=outcome_assessment
                                set_outcome_assessment=set_outcome_assessment
                                outcome_note=outcome_note
                                set_outcome_note=set_outcome_note
                                outcome_occurred_at=outcome_occurred_at
                                set_outcome_occurred_at=set_outcome_occurred_at
                                outcome_busy=outcome_busy
                                on_edit_run=Callback::new(edit_run_outcome)
                                on_save_outcome=Callback::new(save_run_outcome)
                            />
                        </div>
                    </Show>
                </section>
            </Show>

            <Show when=move || workspace_view.get() == WorkspaceView::Analytics && current_project.get().is_some_and(|project| project.published_version_id.is_some())>
                <section id="workspace-panel-analytics" class="decision-analytics-panel" role="tabpanel" aria-labelledby="workspace-tab-analytics" tabindex="0" inert=move || project_busy.get()>
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
                    <details class="analytics-threshold-settings">
                        <summary>"发现阈值"</summary>
                        <div class="analytics-threshold-grid">
                            <label>
                                <span>"失败率预警 (%)"</span>
                                <input
                                    type="number"
                                    min="1"
                                    max="99"
                                    step="1"
                                    prop:value=move || format!("{:.0}", analytics_thresholds.get().failure_warning_rate * 100.0)
                                    on:input=move |event| {
                                        if let Ok(value) = event_target_value(&event).parse::<f64>() {
                                            set_analytics_thresholds.update(|settings| settings.failure_warning_rate = value / 100.0);
                                        }
                                    }
                                    disabled=move || analytics_thresholds_loading.get() || analytics_thresholds_busy.get() || !can_model.get()
                                />
                            </label>
                            <label>
                                <span>"失败率高风险 (%)"</span>
                                <input
                                    type="number"
                                    min="1"
                                    max="100"
                                    step="1"
                                    prop:value=move || format!("{:.0}", analytics_thresholds.get().failure_high_rate * 100.0)
                                    on:input=move |event| {
                                        if let Ok(value) = event_target_value(&event).parse::<f64>() {
                                            set_analytics_thresholds.update(|settings| settings.failure_high_rate = value / 100.0);
                                        }
                                    }
                                    disabled=move || analytics_thresholds_loading.get() || analytics_thresholds_busy.get() || !can_model.get()
                                />
                            </label>
                            <label>
                                <span>"偏差率预警 (%)"</span>
                                <input
                                    type="number"
                                    min="1"
                                    max="99"
                                    step="1"
                                    prop:value=move || format!("{:.0}", analytics_thresholds.get().mismatch_warning_rate * 100.0)
                                    on:input=move |event| {
                                        if let Ok(value) = event_target_value(&event).parse::<f64>() {
                                            set_analytics_thresholds.update(|settings| settings.mismatch_warning_rate = value / 100.0);
                                        }
                                    }
                                    disabled=move || analytics_thresholds_loading.get() || analytics_thresholds_busy.get() || !can_model.get()
                                />
                            </label>
                            <label>
                                <span>"偏差率高风险 (%)"</span>
                                <input
                                    type="number"
                                    min="1"
                                    max="100"
                                    step="1"
                                    prop:value=move || format!("{:.0}", analytics_thresholds.get().mismatch_high_rate * 100.0)
                                    on:input=move |event| {
                                        if let Ok(value) = event_target_value(&event).parse::<f64>() {
                                            set_analytics_thresholds.update(|settings| settings.mismatch_high_rate = value / 100.0);
                                        }
                                    }
                                    disabled=move || analytics_thresholds_loading.get() || analytics_thresholds_busy.get() || !can_model.get()
                                />
                            </label>
                            <label>
                                <span>"失败率最小样本数"</span>
                                <input
                                    type="number"
                                    min="1"
                                    max="100000"
                                    step="1"
                                    prop:value=move || analytics_thresholds.get().failure_min_runs.to_string()
                                    on:input=move |event| {
                                        if let Ok(value) = event_target_value(&event).parse::<i32>() {
                                            set_analytics_thresholds.update(|settings| settings.failure_min_runs = value);
                                        }
                                    }
                                    disabled=move || analytics_thresholds_loading.get() || analytics_thresholds_busy.get() || !can_model.get()
                                />
                            </label>
                            <label>
                                <span>"偏差率最小样本数"</span>
                                <input
                                    type="number"
                                    min="1"
                                    max="100000"
                                    step="1"
                                    prop:value=move || analytics_thresholds.get().mismatch_min_samples.to_string()
                                    on:input=move |event| {
                                        if let Ok(value) = event_target_value(&event).parse::<i32>() {
                                            set_analytics_thresholds.update(|settings| settings.mismatch_min_samples = value);
                                        }
                                    }
                                    disabled=move || analytics_thresholds_loading.get() || analytics_thresholds_busy.get() || !can_model.get()
                                />
                            </label>
                        </div>
                        <Show when=move || analytics_thresholds_error.get().is_some()>
                            <div class="analytics-error">{move || analytics_thresholds_error.get().unwrap_or_default()}</div>
                        </Show>
                        <Show when=move || can_model.get()>
                            <button
                                class="button button-quiet"
                                on:click=save_analytics_thresholds
                                disabled=move || analytics_thresholds_loading.get() || analytics_thresholds_busy.get()
                            >{move || if analytics_thresholds_busy.get() { "保存中..." } else { "保存阈值" }}</button>
                        </Show>
                    </details>
                    <Show when=move || analytics_loading.get() && decision_analytics.get().is_none()>
                        <div class="analytics-state">"正在聚合运行轨迹..."</div>
                    </Show>
                    <Show when=move || analytics_error.get().is_some()>
                        <div class="analytics-error">{move || analytics_error.get().unwrap_or_default()}</div>
                    </Show>
                    {move || decision_analytics.get().map(|report| view! {
                        <analytics::DecisionAnalyticsHighlights
                            summary=report.summary
                            findings=report.findings
                        />
                    })}
                    <analytics::DecisionCopilotPanel
                        token=token
                        current_project=current_project
                        can_route_write=Signal::derive(move || can_model.get() && !workflow_locked.get())
                        on_route_write=route_copilot_write
                    />
                    <section class="optimization-shadow-history" aria-labelledby="optimization-shadow-title">
                        <div class="optimization-shadow-heading">
                            <div>
                                <strong id="optimization-shadow-title">"自动草稿 Shadow 评估"</strong>
                                <small>"只评估试点门槛，不会创建草稿；统计范围为当前已加载记录"</small>
                            </div>
                            <span>{move || shadow_evaluations.get().len()}</span>
                        </div>
                        <Show when=move || shadow_error.get().is_some()>
                            <div class="analytics-error" role="alert">{move || shadow_error.get().unwrap_or_default()}</div>
                        </Show>
                        <Show when=move || !shadow_evaluations.get().is_empty()>
                            <div class="optimization-shadow-summary">
                                <span>{move || {
                                    let evaluations = shadow_evaluations.get();
                                    let passed = evaluations.iter().filter(|item| item.outcome == "eligible_shadow_candidate").count();
                                    format!("门槛通过 {passed}")
                                }}</span>
                                <span>{move || {
                                    let evaluations = shadow_evaluations.get();
                                    let blocked = evaluations.iter().filter(|item| item.outcome != "eligible_shadow_candidate").count();
                                    format!("阻断 {}", blocked)
                                }}</span>
                                <span>{move || format!("已加载 {} 条", shadow_evaluations.get().len())}</span>
                            </div>
                            <div class="optimization-shadow-list">
                                <For each=move || shadow_evaluations.get() key=|item| item.id let:item>
                                    <article>
                                        <div>
                                            <strong>{item.finding_code}</strong>
                                            <span>{format!("模拟 #{} · 候选版本 #{}", item.simulation_id, item.candidate_version_id)}</span>
                                        </div>
                                        <span class=if item.outcome == "eligible_shadow_candidate" { "shadow-outcome is-eligible" } else { "shadow-outcome" }>
                                            {if item.outcome == "eligible_shadow_candidate" { "门槛通过" } else { "已阻断" }}
                                        </span>
                                        <small>{format!("{} · 样本 {} · 一致率 {:.1}% → {:.1}% · {}", item.outcome, item.test_set_size, item.baseline_accuracy, item.simulated_accuracy, item.created_at.format("%Y-%m-%d %H:%M UTC"))}</small>
                                    </article>
                                </For>
                            </div>
                        </Show>
                        <Show when=move || shadow_evaluations.get().is_empty() && !shadow_busy.get() && shadow_error.get().is_none()>
                            <small class="optimization-resolution">"暂无 Shadow 评估记录"</small>
                        </Show>
                        <Show when=move || shadow_has_more.get()>
                            <button class="button button-quiet" on:click=move |_| load_more_shadow_evaluations.run(()) disabled=move || shadow_busy.get()>
                                {move || if shadow_busy.get() { "读取中..." } else { "加载更多评估记录" }}
                            </button>
                        </Show>
                    </section>
                    <section class="drift-rule-panel" aria-labelledby="drift-rule-title">
                        <div class="drift-events-heading">
                            <div>
                                <strong id="drift-rule-title">"漂移扫描规则"</strong>
                                <small>"后台每日检查当前发布版本；命中后只记录待处理事件"</small>
                            </div>
                            <span>{move || if drift_rule_enabled.get() { "已启用" } else { "已停用" }}</span>
                        </div>
                        <Show when=move || drift_rule_error.get().is_some()>
                            <div class="analytics-error">{move || drift_rule_error.get().unwrap_or_default()}</div>
                        </Show>
                        <div class="drift-rule-controls">
                            <Show when=move || can_model.get() fallback=|| view! {
                                <div class="drift-rule-readonly">"当前角色可查看规则，只有项目建模者可以修改。"</div>
                            }>
                                <WorkbenchCheckbox
                                    class="drift-rule-toggle"
                                    label="启用每日漂移扫描"
                                    checked=Signal::derive(move || drift_rule_enabled.get())
                                    disabled=Signal::derive(move || drift_rule_busy.get())
                                    on_change=Callback::new(move |checked| set_drift_rule_enabled.set(checked))
                                    switch=true
                                />
                                <div class="drift-rule-options">
                                    <For
                                        each=move || DRIFT_AUTOMATION_OPTIONS.to_vec()
                                        key=|option| option.0
                                        let:option
                                    >
                                        {
                                            let code = option.0.to_string();
                                            let checked_code = code.clone();
                                            let label = option.1.to_string();
                                            view! {
                                                <WorkbenchCheckbox
                                                    class="drift-rule-option"
                                                    label=label
                                                    checked=Signal::derive(move || drift_rule_codes.get().contains(&checked_code))
                                                    disabled=Signal::derive(move || drift_rule_busy.get())
                                                    on_change=Callback::new(move |checked| {
                                                            let selected_code = code.clone();
                                                            set_drift_rule_codes.update(|codes| {
                                                                if checked && !codes.contains(&selected_code) {
                                                                    codes.push(selected_code);
                                                                    codes.sort();
                                                                } else if !checked {
                                                                    codes.retain(|item| item != &selected_code);
                                                                }
                                                            });
                                                    })
                                                />
                                            }
                                        }
                                    </For>
                                </div>
                                <WorkbenchNumberInput
                                    class="drift-cooldown-field"
                                    label="冷却时间（小时）"
                                    value=drift_rule_cooldown
                                    min=1
                                    max=720
                                    step=1
                                    disabled=Signal::derive(move || drift_rule_busy.get())
                                />
                                <label class="drift-webhook-field">
                                    <span>"Webhook HTTPS 地址"</span>
                                    <input
                                        type="url"
                                        maxlength="2048"
                                        placeholder="https://hooks.example.com/decision-drift"
                                        prop:value=move || drift_webhook_url.get()
                                        on:input=move |event| set_drift_webhook_url.set(event_target_value(&event))
                                    />
                                    <small>"目标主机必须由服务端白名单放行；通知会携带 HMAC-SHA256 签名。留空则不发送通知。"</small>
                                </label>
                                <button
                                    class="button button-secondary"
                                    on:click=save_drift_automation_rule
                                    disabled=move || drift_rule_busy.get()
                                >{move || if drift_rule_busy.get() { "保存中..." } else { "保存扫描规则" }}</button>
                            </Show>
                        </div>
                        <Show when=move || drift_rule.get().is_some_and(|rule| rule.next_eligible_at.is_some())>
                            <small class="drift-rule-next">{move || drift_rule.get().and_then(|rule| rule.next_eligible_at).map(|time| format!("下次可记录事件：{}", time.format("%Y-%m-%d %H:%M UTC"))).unwrap_or_default()}</small>
                        </Show>
                    </section>
                    <Show when=move || !drift_events.get().is_empty() || drift_event_error.get().is_some()>
                        <section class="drift-events" aria-labelledby="drift-events-title">
                            <div class="drift-events-heading">
                                <div>
                                    <strong id="drift-events-title">"漂移事件"</strong>
                                    <small>"事件保留扫描证据；准备提案不会创建草稿或发布版本"</small>
                                </div>
                                <span>{move || drift_events.get().iter().filter(|event| event.status == "open").count()}</span>
                            </div>
                            <Show when=move || drift_event_error.get().is_some()>
                                <div class="analytics-error">{move || drift_event_error.get().unwrap_or_default()}</div>
                            </Show>
                            <div class="drift-event-list">
                                <For
                                    each=move || drift_events.get()
                                    key=|event| event.id
                                    let:event
                                >
                                    {
                                        let event_id = event.id;
                                        let source_version_number = event.evidence["sourceVersionNumber"]
                                            .as_i64()
                                            .unwrap_or(event.source_version_id);
                                        let proposal_ids = event.action_result.as_ref()
                                            .and_then(|result| result.get("proposalIds"))
                                            .and_then(serde_json::Value::as_array)
                                            .map(|ids| ids.iter().filter_map(serde_json::Value::as_i64).collect::<Vec<_>>())
                                            .unwrap_or_default();
                                        let status = match event.status.as_str() {
                                            "proposal_created" => "已生成提案",
                                            "dismissed" => "已忽略",
                                            _ => "待处理",
                                        };
                                        let event_is_open = event.status == "open";
                                        let webhook_status = event.webhook_delivery.as_ref()
                                            .and_then(|delivery| delivery.get("status"))
                                            .and_then(serde_json::Value::as_str)
                                            .unwrap_or("")
                                            .to_string();
                                        let webhook_failed = webhook_status == "failed";
                                        let webhook_error = event.webhook_delivery.as_ref()
                                            .and_then(|delivery| delivery.get("lastError"))
                                            .and_then(serde_json::Value::as_str)
                                            .unwrap_or("")
                                            .to_string();
                                        let review_label = event.review_label.as_deref().unwrap_or("");
                                        let review_text = match review_label {
                                            "confirmed_drift" => "已确认漂移",
                                            "false_positive" => "已标记误报",
                                            "inconclusive" => "判定不确定",
                                            _ => "未人工判定",
                                        };
                                        let reviewed_by = event.reviewed_by.clone().unwrap_or_default();
                                        let reviewed_at = event
                                            .reviewed_at
                                            .as_ref()
                                            .map(|time| time.format("%Y-%m-%d %H:%M UTC").to_string())
                                            .unwrap_or_default();
                                        view! {
                                            <article class="drift-event-row">
                                                <div class="drift-event-main">
                                                    <div class="drift-event-title">
                                                        <strong>{event.finding_codes.join(" · ")}</strong>
                                                        <span>{status}</span>
                                                    </div>
                                                    <small>{format!("发布版本 v{} · {}", source_version_number, event.observed_at.format("%Y-%m-%d %H:%M UTC"))}</small>
                                                    <p>{event.evidence["findings"].as_array().into_iter().flatten().filter_map(|finding| finding["message"].as_str()).collect::<Vec<_>>().join("；")}</p>
                                                    <small>{if reviewed_by.is_empty() { review_text.into() } else { format!("人工判定：{} · {} · {}", review_text, reviewed_by, reviewed_at) }}</small>
                                                    {(!webhook_status.is_empty()).then(|| view! {
                                                        <small>{format!("通知状态：{}（第 {} 次尝试）", match webhook_status.as_str() { "pending" | "sending" => "投递中", "delivered" => "已送达", "failed" => "失败", _ => "未知" }, event.webhook_delivery.as_ref().and_then(|delivery| delivery.get("attemptCount")).and_then(serde_json::Value::as_i64).unwrap_or(0))}</small>
                                                    })}
                                                    {(!webhook_error.is_empty()).then(|| view! { <small>{format!("通知错误：{webhook_error}")}</small> })}
                                                    <button
                                                        class="button button-ghost"
                                                        on:click=move |_| load_drift_event_impact.run(event_id)
                                                        disabled=move || drift_impact_busy.get().is_some()
                                                    >{move || if drift_impact_busy.get() == Some(event_id) { "读取中..." } else { "查看近 30 天影响" }}</button>
                                                    {move || drift_impacts.get().get(&event_id).cloned().map(|report| view! {
                                                        <div class="drift-impact-report" role="status">
                                                            <small>{format!("版本记录 #{} 近 30 天共运行 {} 次：成功 {}，失败 {}；有效反馈 {}，其中偏差 {}。", report.source_version_id, report.total_runs, report.succeeded_runs, report.failed_runs, report.feedback_runs, report.mismatch_runs)}</small>
                                                            <For each=move || report.elements.clone() key=|impact| impact.element.clone() let:impact>
                                                                <small>{format!("{}：命中 {} 次，成功 {} 次，反馈 {} 次，偏差 {} 次；规则 {}", impact.element, impact.observed_runs, impact.succeeded_runs, impact.feedback_runs, impact.mismatch_runs, if impact.matched_rule_ids.is_empty() { "无规则命中明细".into() } else { impact.matched_rule_ids.join(", ") })}</small>
                                                            </For>
                                                        </div>
                                                    })}
                                                    {(!proposal_ids.is_empty()).then(|| view! {
                                                        <small>{format!("提案 ID：{}", proposal_ids.iter().map(i64::to_string).collect::<Vec<_>>().join(", "))}</small>
                                                    })}
                                                </div>
                                                <Show when=move || event_is_open || webhook_failed || can_model.get()>
                                                    <div class="drift-event-actions">
                                                        <Show when=move || can_model.get()>
                                                            <button
                                                                class="button button-ghost"
                                                                on:click=move |_| review_drift_event.run((event_id, "confirmed_drift".into()))
                                                                disabled=move || drift_event_busy.get()
                                                            >"确认漂移"</button>
                                                            <button
                                                                class="button button-ghost"
                                                                on:click=move |_| review_drift_event.run((event_id, "false_positive".into()))
                                                                disabled=move || drift_event_busy.get()
                                                            >"标记误报"</button>
                                                            <button
                                                                class="button button-ghost"
                                                                on:click=move |_| review_drift_event.run((event_id, "inconclusive".into()))
                                                                disabled=move || drift_event_busy.get()
                                                            >"不确定"</button>
                                                            <Show when=move || webhook_failed>
                                                                <button
                                                                    class="button button-ghost"
                                                                    on:click=move |_| retry_drift_webhook.run(event_id)
                                                                    disabled=move || drift_event_busy.get()
                                                                >{move || if drift_event_busy.get() { "处理中..." } else { "重试通知" }}</button>
                                                            </Show>
                                                            <Show when=move || event_is_open>
                                                            <button
                                                                class="button button-secondary"
                                                                on:click=move |_| prepare_drift_event_proposals.run(event_id)
                                                                disabled=move || drift_event_busy.get()
                                                            >{move || if drift_event_busy.get() { "处理中..." } else { "准备提案" }}</button>
                                                            <button
                                                                class="button button-ghost"
                                                                on:click=move |_| dismiss_drift_event.run(event_id)
                                                                disabled=move || drift_event_busy.get()
                                                            >"忽略"</button>
                                                            </Show>
                                                        </Show>
                                                    </div>
                                                </Show>
                                            </article>
                                        }
                                    }
                                </For>
                            </div>
                        </section>
                    </Show>
                    <Show when=move || !drift_scan_windows.get().is_empty() || drift_event_error.get().is_some()>
                        <section class="drift-events" aria-labelledby="drift-windows-title">
                            <div class="drift-events-heading">
                                <div>
                                    <strong id="drift-windows-title">"每日扫描窗口"</strong>
                                    <small>"包含未触发事件的完整分析快照；人工标签用于阈值回放"</small>
                                </div>
                                <span>{move || drift_scan_windows.get().len()}</span>
                            </div>
                            <div class="drift-event-list">
                                <For
                                    each=move || newest_drift_scan_windows(drift_scan_windows.get())
                                    key=|window| window.id
                                    let:window
                                >
                                    {
                                        let window_id = window.id;
                                        let report = &window.evidence;
                                        let findings = report["findings"].as_array()
                                            .into_iter().flatten()
                                            .filter_map(|finding| finding["code"].as_str())
                                            .collect::<Vec<_>>();
                                        let finding_label = if findings.is_empty() { "无触发信号".to_string() } else { findings.join(" · ") };
                                        let summary = &report["summary"];
                                        let run_count = summary["totalRuns"].as_i64().unwrap_or(0);
                                        let outcome_count = summary["conclusiveOutcomeRuns"].as_i64().unwrap_or(0);
                                        let failure_rate = summary["failureRate"].as_f64().unwrap_or(0.0) * 100.0;
                                        let mismatch_rate = summary["mismatchRate"].as_f64().unwrap_or(0.0) * 100.0;
                                        let label = window.review_label.as_deref().unwrap_or("");
                                        let label_text = match label {
                                            "drift" => "确认漂移",
                                            "normal" => "正常窗口",
                                            "inconclusive" => "不确定",
                                            _ => "待人工判定",
                                        };
                                        let reviewer = window.reviewed_by.clone().unwrap_or_default();
                                        let reviewed_at = window.reviewed_at
                                            .map(|time| time.format("%Y-%m-%d %H:%M UTC").to_string())
                                            .unwrap_or_default();
                                        view! {
                                            <article class="drift-event-row">
                                                <div class="drift-event-main">
                                                    <div class="drift-event-title">
                                                        <strong>{window.scan_day.format("%Y-%m-%d").to_string()}</strong>
                                                        <span>{finding_label}</span>
                                                    </div>
                                                    <small>{format!("发布版本 #{} · {} 次运行 · {} 条有效反馈", window.source_version_id, run_count, outcome_count)}</small>
                                                    <small>{format!("失败率 {:.1}% · 偏差率 {:.1}%", failure_rate, mismatch_rate)}</small>
                                                    <small>{if reviewer.is_empty() { label_text.to_string() } else { format!("人工判定：{label_text} · {reviewer} · {reviewed_at}") }}</small>
                                                    <p>{report["findings"].as_array().into_iter().flatten().filter_map(|finding| finding["message"].as_str()).collect::<Vec<_>>().join("；")}</p>
                                                </div>
                                                <Show when=move || can_model.get()>
                                                    <div class="drift-event-actions">
                                                        <button class="button button-ghost" on:click=move |_| review_drift_scan_window.run((window_id, "drift".into())) disabled=move || drift_event_busy.get()>"确认漂移"</button>
                                                        <button class="button button-ghost" on:click=move |_| review_drift_scan_window.run((window_id, "normal".into())) disabled=move || drift_event_busy.get()>"标记正常"</button>
                                                        <button class="button button-ghost" on:click=move |_| review_drift_scan_window.run((window_id, "inconclusive".into())) disabled=move || drift_event_busy.get()>"不确定"</button>
                                                    </div>
                                                </Show>
                                            </article>
                                        }
                                    }
                                </For>
                            </div>
                        </section>
                    </Show>
                    <Show when=move || !deviation_analyses.get().is_empty() || drift_event_error.get().is_some()>
                        <section class="drift-events" aria-labelledby="deviation-review-title">
                            <div class="drift-events-heading">
                                <div>
                                    <strong id="deviation-review-title">"偏差归因抽检"</strong>
                                    <small>"最近 100 条自动归因候选；不展示业务输入或输出值"</small>
                                </div>
                                <span>{move || deviation_analyses.get().len()}</span>
                            </div>
                            <div class="drift-event-list">
                                <For each=move || deviation_analyses.get() key=|analysis| analysis.id let:analysis>
                                    {
                                        let analysis_id = analysis.id;
                                        let label = analysis.review_label.as_deref().unwrap_or("");
                                        let review_text = match label {
                                            "correct" => "归因正确",
                                            "incorrect" => "归因错误",
                                            "inconclusive" => "证据不足",
                                            _ => "待人工判定",
                                        };
                                        let reviewer = analysis.reviewed_by.clone().unwrap_or_default();
                                        let reviewed_at = analysis.reviewed_at
                                            .map(|time| time.format("%Y-%m-%d %H:%M UTC").to_string())
                                            .unwrap_or_default();
                                        let is_candidate = analysis.status == "candidate";
                                        let status = if is_candidate { "候选规则" } else { "证据不足" };
                                        let rule = analysis.attributed_rule.clone().unwrap_or_else(|| "未定位候选规则".into());
                                        view! {
                                            <article class="drift-event-row">
                                                <div class="drift-event-main">
                                                    <div class="drift-event-title">
                                                        <strong>{format!("运行 #{} · {}", analysis.decision_run_id, rule)}</strong>
                                                        <span>{status}</span>
                                                    </div>
                                                    <small>{format!("候选置信度 {:.0}% · {}", analysis.confidence * 100.0, analysis.created_at.format("%Y-%m-%d %H:%M UTC"))}</small>
                                                    <small>{if reviewer.is_empty() { review_text.to_string() } else { format!("人工判定：{review_text} · {reviewer} · {reviewed_at}") }}</small>
                                                </div>
                                                <Show when=move || can_model.get()>
                                                    <div class="drift-event-actions">
                                                        <Show when=move || is_candidate>
                                                            <button class="button button-ghost" on:click=move |_| review_deviation_analysis.run((analysis_id, "correct".into())) disabled=move || drift_event_busy.get()>"归因正确"</button>
                                                            <button class="button button-ghost" on:click=move |_| review_deviation_analysis.run((analysis_id, "incorrect".into())) disabled=move || drift_event_busy.get()>"归因错误"</button>
                                                        </Show>
                                                        <button class="button button-ghost" on:click=move |_| review_deviation_analysis.run((analysis_id, "inconclusive".into())) disabled=move || drift_event_busy.get()>"证据不足"</button>
                                                    </div>
                                                </Show>
                                            </article>
                                        }
                                    }
                                </For>
                            </div>
                        </section>
                    </Show>
                    <Show when=move || !optimization_proposals.get().is_empty() || proposal_error.get().is_some()>
                        <div class="optimization-proposals" id="optimization-proposals">
                            <div class="optimization-proposals-heading">
                                <div>
                                    <strong>"优化建议"</strong>
                                    <small>"建议基于发布版本运行快照；确认仅创建草稿，不会自动修改或发布模型"</small>
                                    <small>{move || {
                                        let proposals = optimization_proposals.get();
                                        let total = proposals.len();
                                        let drafts = proposals
                                            .iter()
                                            .filter(|proposal| {
                                                proposal.status == "confirmed"
                                                    && proposal.resulting_version_id.is_some()
                                            })
                                            .count();
                                        let rate = if total == 0 {
                                            0.0
                                        } else {
                                            drafts as f64 / total as f64 * 100.0
                                        };
                                        format!("累计草稿转化 {drafts}/{total} · {rate:.1}%")
                                    }}</small>
                                </div>
                                <span>{move || optimization_proposals.get().iter().filter(|proposal| proposal.status == "open").count()}</span>
                            </div>
                            <div class="simulation-version-picker">
                                <WorkbenchSelect
                                    label="效果模拟候选版本"
                                    class="optimization-version-select"
                                    options=Signal::derive(move || {
                                        versions
                                            .get()
                                            .into_iter()
                                            .map(|version| SelectOption::new(
                                                version.id.to_string(),
                                                format!("v{} · {}", version.version_number, version.status),
                                            ))
                                            .collect()
                                    })
                                    value=Signal::derive(move || simulation_candidate_version_id.get().map(|id| id.to_string()).unwrap_or_default())
                                    disabled=Signal::derive(move || versions.get().is_empty())
                                    placeholder=Signal::derive(|| "请选择候选版本".to_string())
                                    on_change=Callback::new(move |value: String| {
                                        set_simulation_candidate_version_id.set(value.parse::<i64>().ok());
                                        set_optimization_simulation.set(None);
                                    })
                                />
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
                                        let simulation_id = proposal.id;
                                        let proposal_key = proposal.id;
                                        let is_open = proposal.status == "open";
                                        let proposal_prompt =
                                            optimization_proposal_handoff_prompt(&proposal);
                                        let proposal_prompt = RwSignal::new(proposal_prompt);
                                        let route_proposal = route_copilot_write;
                                        let can_confirm = move || {
                                            optimization_proposals
                                                .get()
                                                .iter()
                                                .any(|item| {
                                                    item.id == proposal_key
                                                        && item.latest_simulation_passed
                                                })
                                        };
                                        let resolution = if proposal.status == "confirmed" {
                                            proposal
                                                .resulting_version_id
                                                .map(|id| format!("已创建草稿 #{id}"))
                                                .unwrap_or_default()
                                        } else if proposal.status == "withdrawn" {
                                            proposal.resolution_note.clone()
                                        } else {
                                            "已保留人工驳回记录".into()
                                        };
                                        let effectiveness_summary = proposal
                                            .effectiveness_result
                                            .as_ref()
                                            .map(|result| {
                                                let window_summary = |label: &str, key: &str| {
                                                    let window = &result[key];
                                                    let baseline = window["baselineSampleCount"]
                                                        .as_u64()
                                                        .unwrap_or_default();
                                                    let candidate = window["candidateSampleCount"]
                                                        .as_u64()
                                                        .unwrap_or_default();
                                                    match window["status"].as_str() {
                                                        Some("complete") => format!(
                                                            "{label}预测 {:+.2}pp，实际 {:+.2}pp，误差 {:.2}pp",
                                                            window["predictedImprovementPp"]
                                                                .as_f64()
                                                                .unwrap_or_default(),
                                                            window["actualImprovementPp"]
                                                                .as_f64()
                                                                .unwrap_or_default(),
                                                            window["absoluteErrorPp"]
                                                                .as_f64()
                                                                .unwrap_or_default(),
                                                        ),
                                                        Some("pending") => {
                                                            format!("{label}观察窗口尚未结束")
                                                        }
                                                        _ => format!(
                                                            "{label}样本积累中（基线 {baseline}，发布后 {candidate}）"
                                                        ),
                                                    }
                                                };
                                                if result.get("sevenDay").is_some() {
                                                    format!(
                                                        "效果追踪：{}；{}。",
                                                        window_summary("7 天", "sevenDay"),
                                                        window_summary("30 天", "thirtyDay"),
                                                    )
                                                } else {
                                                    format!(
                                                        "30 天效果追踪：预测改善 {:+.2} 个百分点，实际改善 {:+.2} 个百分点，误差 {:.2} 个百分点；反馈样本 {}/{}。",
                                                        result["predictedImprovementPp"].as_f64().unwrap_or_default(),
                                                        result["actualImprovementPp"].as_f64().unwrap_or_default(),
                                                        result["absoluteErrorPp"].as_f64().unwrap_or_default(),
                                                        result["baselineSampleCount"].as_u64().unwrap_or_default(),
                                                        result["candidateSampleCount"].as_u64().unwrap_or_default(),
                                                    )
                                                }
                                            });
                                        let representative_runs = proposal.evidence["representativeRuns"]
                                            .as_array()
                                            .cloned()
                                            .unwrap_or_default()
                                            .into_iter()
                                            .filter_map(|run| {
                                                let run_id = run["runId"].as_i64()?;
                                                let status = run["status"].as_str().unwrap_or("unknown");
                                                let assessment = run["outcomeAssessment"]
                                                    .as_str()
                                                    .unwrap_or("未回填");
                                                let fields = |key: &str| {
                                                    run[key]
                                                        .as_array()
                                                        .into_iter()
                                                        .flatten()
                                                        .filter_map(|field| field.as_str())
                                                        .collect::<Vec<_>>()
                                                        .join("、")
                                                };
                                                Some(format!(
                                                    "运行 #{} · {} · {} · 输入字段：{} · 输出字段：{}",
                                                    run_id,
                                                    status,
                                                    assessment,
                                                    fields("inputFields"),
                                                    fields("outputFields"),
                                                ))
                                            })
                                            .collect::<Vec<_>>();
                                        let representative_runs = RwSignal::new(representative_runs);
                                        let resolution = RwSignal::new(resolution);
                                        view! {
                                            <article class=format!("optimization-proposal proposal-{}", proposal.finding_level)>
                                                <div class="optimization-proposal-title">
                                                    <div><strong>{proposal.title}</strong><small>{format!("v{} · {} · {}", proposal.source_version_number, proposal.finding_code, proposal.generated_at.format("%m-%d %H:%M"))}</small></div>
                                                    <span class=format!("proposal-status status-{}", proposal.status)>{optimization_proposal_status_label(&proposal.status)}</span>
                                                </div>
                                                <p>{proposal.summary}</p>
                                                <Show when=move || !representative_runs.get().is_empty()>
                                                    <div class="optimization-evidence">
                                                        <strong>"代表运行证据（仅字段摘要）"</strong>
                                                        <For each=move || representative_runs.get() key=|run| run.clone() let:run>
                                                            <small>{run}</small>
                                                        </For>
                                                    </div>
                                                </Show>
                                                <ul>
                                                    <For each=move || actions.clone() key=|action| action.clone() let:action>
                                                        <li>{action}</li>
                                                    </For>
                                                </ul>
                                                <Show when=move || is_open>
                                                    <div class="optimization-proposal-actions">
                                                        <button
                                                            class="button button-quiet"
                                                            on:click=move |_| route_proposal.run(proposal_prompt.get())
                                                            disabled=move || !can_model.get() || workflow_locked.get()
                                                        >"按建议分析变更"</button>
                                                        <button class="button button-primary" on:click=move |_| confirm_optimization_proposal(confirm_id) disabled=move || proposal_busy.get() || !can_confirm()>"确认并创建草稿"</button>
                                                        <button class="button button-quiet" on:click=move |_| dismiss_optimization_proposal(dismiss_id) disabled=move || proposal_busy.get()>"驳回"</button>
                                                    </div>
                                                </Show>
                                                <Show when=move || is_open && !can_confirm()>
                                                    <small class="optimization-resolution">"请先运行候选版本模拟；通过后才能确认并创建草稿。"</small>
                                                </Show>
                                                <button
                                                    class="button button-quiet"
                                                    disabled=move || simulation_busy.get() || simulation_candidate_version_id.get().is_none()
                                                    on:click=move |_| simulate_optimization.run(simulation_id)
                                                >
                                                    {move || if simulation_busy.get() { "模拟中..." } else { "模拟候选版本效果" }}
                                                </button>
                                                <Show when=move || !is_open>
                                                    <small class="optimization-resolution">{move || resolution.get()}</small>
                                                </Show>
                                                {effectiveness_summary.map(|summary| view! {
                                                    <small class="optimization-resolution">{summary}</small>
                                                })}
                                            </article>
                                        }
                                    }
                                </For>
                            </div>
                            <Show when=move || optimization_simulation.get().is_some()>
                                <div class="optimization-simulation-result">
                                    {move || optimization_simulation.get().map(|report| view! {
                                        <>
                                            <strong>{format!("效果模拟 · {} 个样本", report.test_set_size)}</strong>
                                            <strong>{if report.passed { "模拟通过" } else if report.test_set_size < report.minimum_sample_count { "样本不足" } else { "模拟未通过" }}</strong>
                                            <span>{format!("基线 v{} 一致率 {:.2}% → 候选 v{} {:.2}%（{:+.2} 个百分点）", report.baseline_version_id, report.baseline_accuracy, report.candidate_version_id, report.simulated_accuracy, report.accuracy_improvement)}</span>
                                            <span>{format!("偏差率：{:.2}% → {:.2}%", report.bias_rate_baseline, report.bias_rate_simulated)}</span>
                                            <span>{if report.sample_source == "approved_business_tests" { "样本来源：已确认业务验收案例" } else { "样本来源：成功历史执行记录（行为基线，不代表业务真值）" }}</span>
                                            <small>{report.estimated_business_impact}</small>
                                            {report.shadow_evaluation.map(|outcome| view! {
                                                <small>{match outcome.as_str() {
                                                    "eligible_shadow_candidate" if report.autodraft_created_version_id.is_some() => "自动草稿已创建；仍需完成业务验收、独立复核和人工发布。".to_string(),
                                                    "eligible_shadow_candidate" if report.autodraft_error.is_some() => "自动草稿创建失败；已记录审计，请人工处理。".to_string(),
                                                    "eligible_shadow_candidate" => "自动草稿门控通过；当前为 Shadow 模式，仅记录结果。".to_string(),
                                                    "blocked_project_risk" => "Shadow 未通过：项目风险等级不符合试点范围。".to_string(),
                                                    "blocked_finding_code" => "Shadow 未通过：发现类型不在试点白名单。".to_string(),
                                                    "blocked_unlabeled_samples" => "Shadow 未通过：样本不是已批准业务案例。".to_string(),
                                                    "blocked_insufficient_samples" => "Shadow 未通过：业务案例数量不足。".to_string(),
                                                    "blocked_simulation" => "Shadow 未通过：候选模拟未通过。".to_string(),
                                                    _ => "Shadow 未通过：变更风险不符合试点门槛。".to_string(),
                                                }}</small>
                                            })}
                                            {report.autodraft_created_version_id.map(|version_id| view! {
                                                <small>{format!("自动创建的草稿版本 ID：{version_id}；该草稿未验收、未复核、未发布。")}</small>
                                            })}
                                        </>
                                    })}
                                </div>
                            </Show>
                        </div>
                    </Show>
                    <analytics::DecisionAnalyticsMetrics report=decision_analytics runs=service_runs />
                </section>
            </Show>

            <Show when=move || workspace_view.get() == WorkspaceView::Validation>
            <div id="workspace-panel-validation" role="tabpanel" aria-labelledby="workspace-tab-validation" tabindex="0">
            <Show when=move || pending_review.get().is_some() && can_model.get()>
                <div class="model-change-lock" role="status">
                    <strong>"请先处理待确认的模型变更"</strong>
                    <button class="button button-quiet" on:click=move |_| set_workspace_view.set(WorkspaceView::Model)>"返回建模查看变更"</button>
                </div>
            </Show>
            <Show when=move || current_version.get().is_some() && (pending_review.get().is_none() || !can_model.get())>
                <section class="acceptance-workspace" aria-label="案例验收工作区">
                <div class="acceptance-workspace-tabs" role="tablist" aria-label="验收工作区内容">
                    <button
                        type="button"
                        role="tab"
                        id="acceptance-tab-cases"
                        aria-controls="acceptance-panel-cases"
                        aria-selected=move || acceptance_tab.get() == "cases"
                        class=move || if acceptance_tab.get() == "cases" { "acceptance-workspace-tab is-active" } else { "acceptance-workspace-tab" }
                        on:click=move |_| set_acceptance_tab.set("cases".into())
                    >
                        <span>"业务验收测试套件"</span>
                        <b>{move || decision_test_cases.get().len()}</b>
                    </button>
                    <button
                        type="button"
                        role="tab"
                        id="acceptance-tab-proposals"
                        aria-controls="acceptance-panel-proposals"
                        aria-selected=move || acceptance_tab.get() == "proposals"
                        class=move || if acceptance_tab.get() == "proposals" { "acceptance-workspace-tab is-active" } else { "acceptance-workspace-tab" }
                        on:click=move |_| set_acceptance_tab.set("proposals".into())
                    >
                        <span>"测试工程提案"</span>
                        <b>{move || decision_workflow.get().and_then(|workflow| workflow.tester_report).map(|report| report.proposals.len()).unwrap_or(0)}</b>
                    </button>
                </div>
                <div
                    id="acceptance-panel-proposals"
                    class="acceptance-tab-panel"
                    role="tabpanel"
                    aria-labelledby="acceptance-tab-proposals"
                    hidden=move || acceptance_tab.get() != "proposals"
                >
                <Show
                    when=move || decision_workflow.get().and_then(|workflow| workflow.tester_report).is_some()
                    fallback=|| view! { <div class="acceptance-tab-empty">"完成需求分析与方案规划后，测试工程提案会显示在这里。"</div> }
                >
                    {move || decision_workflow.get().and_then(|workflow| workflow.tester_report).map(|report| {
                        let project = current_project.get();
                        let draft_version_id = project.as_ref().and_then(|project| {
                            current_version.get().filter(|version| {
                                project.current_draft_version_id == Some(version.id)
                            }).map(|version| version.id)
                        });
                        let project_id = project.map(|project| project.id);
                        view! {
                            <TesterReportView
                                report=report
                                project_id=project_id
                                version_id=draft_version_id
                                token=token.get()
                                can_model=can_model
                                on_converted=Callback::new(move |test_case: DecisionTestCase| {
                                    set_decision_test_cases.update(|cases| {
                                        if !cases.iter().any(|existing| existing.id == test_case.id) {
                                            cases.push(test_case.clone());
                                        }
                                    });
                                    set_verification_report.set(None);
                                    set_publish_confirmed.set(false);
                                    set_workspace_view.set(WorkspaceView::Validation);
                                    mark_version_draft(test_case.version_id);
                                    set_version_save_status.set(Some("Tester 提案已转为待执行用例，请运行并交由复核者批准".into()));
                                    set_audit_revision.update(|revision| *revision += 1);
                                })
                            />
                        }
                    })}
                </Show>
                </div>
                <div
                    id="acceptance-panel-cases"
                    class="acceptance-tab-panel"
                    role="tabpanel"
                    aria-labelledby="acceptance-tab-cases"
                    hidden=move || acceptance_tab.get() != "cases"
                >
                <section class="business-tests-panel" aria-label="业务验收测试套件">
                    <div class="business-tests-heading">
                        <div>
                            <span class="yaml-kicker">"BUSINESS ACCEPTANCE"</span>
                            <strong>"业务验收测试套件"</strong>
                            <small>{move || {
                                let cases = decision_test_cases.get();
                                let selected = selected_test_case_ids.get().len();
                                format!("{} 个案例 · 已选 {}", cases.len(), selected)
                            }}</small>
                        </div>
                        <div class="business-tests-heading-actions">
                            <button
                                class="button button-quiet"
                                on:click=select_pending_test_cases
                                disabled=move || test_case_busy.get() || !decision_test_cases.get().iter().any(|case| case.status == "passed" && case.approved_by.is_none())
                            >"选中全部待批准"</button>
                            <button
                                class="button button-quiet"
                                on:click=move |_| set_selected_test_case_ids.set(HashSet::new())
                                disabled=move || test_case_busy.get() || selected_test_case_ids.get().is_empty()
                            >"清空选择"</button>
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
                            <Show when=move || can_model.get()>
                                <button
                                    class="button button-quiet"
                                    on:click=generate_business_test_cases
                                    disabled=move || test_case_busy.get()
                                        || current_version.get().is_some_and(|version| matches!(version.status.as_str(), "published" | "retired"))
                                >
                                    {move || if test_case_running.get() { "案例执行中" } else if test_case_busy.get() { "处理中..." } else { "生成候选案例" }}
                                </button>
                            </Show>
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
                    <BusinessTestCaseSummary
                        cases=decision_test_cases
                        filter=test_case_filter
                        set_filter=set_test_case_filter
                    />
                    <div class="business-test-approval-hint">
                        <span>"待批准仅包含已通过且未批准的案例；批准权限受项目复核角色和版本创建者限制。"</span>
                        <span role="status">{move || {
                            let selected = selected_test_case_ids.get();
                            let filter = test_case_filter.get();
                            let cases = decision_test_cases.get();
                            let visible = cases.iter().filter(|case| filter.matches(case)).count();
                            let hidden_selected = cases.iter().filter(|case| selected.contains(&case.id) && !filter.matches(case)).count();
                            format!("显示 {} / {} 个 · 已选 {} 个（筛选外 {} 个）。批量操作包含筛选外已选项，运行全部不受筛选影响。", visible, cases.len(), selected.len(), hidden_selected)
                        }}</span>
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
                    <Show when=move || can_model.get() && current_version.get().is_some_and(|version| !matches!(version.status.as_str(), "published" | "retired"))>
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
                    <div class="business-test-list" id="business-test-case-list">
                        <Show
                            when=move || !decision_test_cases.get().is_empty()
                            fallback=move || view! {
                                <div class="business-test-list-empty">
                                    "暂无业务验收案例。可点击“生成候选案例”或手工添加案例。"
                                </div>
                            }
                        >
                            <Show
                                when=move || !decision_test_cases.get().iter().any(|case| test_case_filter.get().matches(case))
                            >
                                    <div class="business-test-list-empty is-filter-empty" role="status">
                                        {move || format!("当前筛选“{}”没有匹配案例。", test_case_filter.get().label())}
                                        <button class="button button-quiet" on:click=move |_| set_test_case_filter.set(TestCaseFilter::All)>"查看全部案例"</button>
                                    </div>
                            </Show>
                                <For
                                    each=move || decision_test_cases.get()
                                    key=|test_case| format!(
                                        "{}:{}:{}:{}:{}",
                                        test_case.id,
                                        test_case.status,
                                        test_case.approved_by.is_some(),
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
                                    let source_label = match test_case.source.as_str() {
                                        "generated" => "规则生成 · 待业务核对",
                                        "tester_proposal" => "Tester 提案 · 业务填写",
                                        _ => "人工录入",
                                    };
                                    let can_approve = status == "passed";
                                    let can_select = can_approve;
                                    let input = serde_json::to_string_pretty(&test_case.input_json).unwrap_or_default();
                                    let expected = serde_json::to_string_pretty(&test_case.expected_output_json).unwrap_or_default();
                                    let is_failed = TestCaseFilter::Failed.matches(&test_case);
                                    let is_pending_approval = TestCaseFilter::PendingApproval.matches(&test_case);
                                    let is_pending_run = TestCaseFilter::PendingRun.matches(&test_case);
                                    let selection_label = format!("选择验收案例：{}", test_case.name);
                                    let actual = test_case.actual_output_json
                                        .as_ref()
                                        .and_then(|value| serde_json::to_string_pretty(value).ok())
                                        .unwrap_or_else(|| "尚未运行".into());
                                    let trace = test_case.execution_trace_json
                                        .and_then(|value| serde_json::from_value::<ExecutionTrace>(value).ok());
                                    view! {
                                        <article
                                            class=move || format!("business-test-case case-{status_for_class}{}", if test_case_running.get() { " is-running" } else { "" })
                                            hidden=move || !match test_case_filter.get() {
                                                TestCaseFilter::All => true,
                                                TestCaseFilter::Failed => is_failed,
                                                TestCaseFilter::PendingApproval => is_pending_approval,
                                                TestCaseFilter::PendingRun => is_pending_run,
                                            }
                                            aria-busy=move || test_case_running.get()
                                    >
                                        <div class="business-test-case-heading">
                                            <div class="business-test-case-heading-title">
                                                <label class="business-test-select" title="选择此验收案例">
                                                    <input
                                                        type="checkbox"
                                                        aria-label=selection_label
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
                                                <small class=match test_case.source.as_str() {
                                                    "generated" => "case-source generated",
                                                    "tester_proposal" => "case-source tester-proposal",
                                                    _ => "case-source manual",
                                                }>
                                                    {source_label}
                                                </small>
                                            </div>
                                            <em>{if approved { "已批准" } else if can_approve { "待批准" } else { "通过后可批准" }}</em>
                                        </div>
                                        <details class="business-test-details" prop:open=matches!(status.as_str(), "failed" | "error")>
                                            <summary>{match status.as_str() {
                                                "passed" => "实际输出符合期望 · 查看输入与结果",
                                                "failed" => "实际输出与期望不一致 · 查看输入与结果",
                                                "error" => "案例执行出错 · 查看输入与结果",
                                                _ => "尚未执行 · 查看输入与期望",
                                            }}</summary>
                                            <div class="business-test-values">
                                                <div class="business-test-value value-input"><small>"输入"</small><code inner_html=move || highlight_code(&input, "json")></code></div>
                                                <div class="business-test-value value-expected"><small>"期望"</small><code inner_html=move || highlight_code(&expected, "json")></code></div>
                                                <div class="business-test-value value-actual"><small>"实际"</small><code inner_html=move || highlight_code(&actual, "json")></code></div>
                                            </div>
                                        </details>
                                        <div class="business-test-actions">
                                            <button
                                                class="button button-quiet"
                                                on:click=move |_| approve_business_test_case(case_id, !approved)
                                                title=move || if !can_review.get() { "仅复核者可批准业务验收案例" } else if !can_approve { "请先运行案例，只有执行通过的案例可以批准" } else if approved { "撤销此案例批准" } else { "批准此案例" }
                                                disabled=move || test_case_busy.get()
                                                    || !can_review.get()
                                                    || !can_approve
                                                    || current_version.get().is_some_and(|version| matches!(version.status.as_str(), "published" | "retired"))
                                            >{if approved { "撤销批准" } else { "批准" }}</button>
                                            <Show when=move || can_model.get()>
                                                <button
                                                    class="button button-quiet danger-action"
                                                    on:click=move |_| delete_business_test_case(case_id)
                                                    disabled=move || test_case_busy.get()
                                                        || current_version.get().is_some_and(|version| matches!(version.status.as_str(), "published" | "retired"))
                                                >"删除"</button>
                                            </Show>
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
                </div>
                </section>
            </Show>

            <Show when=move || workspace_view.get() == WorkspaceView::Validation && current_version.get().is_some() && (pending_review.get().is_none() || !can_model.get())>
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
                                on_view_change=move |_| {
                                    if let Some(version) = current_version.get() {
                                        select_version_review(version.id.to_string());
                                    }
                                }
                                busy=verification_busy
                                published=published
                                action_error=release_error
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
            <Show when=move || workspace_view.get() == WorkspaceView::Validation && verification_report.get().is_none() && release_error.get().is_some()>
                <div class="release-error">{move || release_error.get().unwrap_or_default()}</div>
            </Show>
            </div>
            </Show>

            <Show when=move || workspace_view.get() == WorkspaceView::Model || workspace_view.get() == WorkspaceView::Discovery>
            <div class=move || if workspace_view.get() == WorkspaceView::Discovery { "model-workspace discovery-workspace" } else { "model-workspace" }>
                <Show when=move || workspace_view.get() == WorkspaceView::Discovery>
                    <section id="workspace-panel-discovery" class="workflow-panel workflow-stage-panel" role="tabpanel" aria-labelledby="workspace-tab-discovery" tabindex="0" inert=move || project_busy.get()>
                        <div class="workflow-heading">
                            <div>
                                <span class="yaml-kicker">"DECISION DISCOVERY"</span>
                                <strong>{move || if decision_workflow.get().is_some() { "需求分析结果" } else { "需求分析" }}</strong>
                                <small class="workflow-heading-description">"先核对业务目标和关键规则，再补充需要确认的信息"</small>
                            </div>
                            <span class=move || format!("workflow-state state-{}", if workflow_analyzing.get() { "analyzing".to_string() } else if workflow_planning.get() { "planning".into() } else if workflow_confirming.get() { "confirming".into() } else if !workflow_current.get() && decision_workflow.get().is_some() { "new".into() } else { decision_workflow.get().map(|workflow| workflow.status).unwrap_or_else(|| "new".into()) })>
                                {move || if workflow_analyzing.get() { "分析中" } else if workflow_planning.get() { "规划中" } else if workflow_confirming.get() { "确认中" } else if !workflow_current.get() && decision_workflow.get().is_some() { "待重新分析" } else if workflow_answers_changed.get() { "待重新规划" } else { workflow_status_label(decision_workflow.get().as_ref().map(|workflow| workflow.status.as_str()).unwrap_or("new")) }}
                            </span>
                        </div>
                        <div class="workflow-chain-context">
                            <strong>{move || current_version.get().map(|version| format!("需求变更 · 基于草稿 v{}", version.version_number)).unwrap_or_else(|| "新建决策模型".into())}</strong>
                            <span>"需求分析 → 规划并确认方案 → 生成 DSL → 编译 DMN"</span>
                        </div>
                        <Show when=move || current_version.get().is_some() && decision_workflow.get().is_some_and(|workflow| workflow.original_requirement_text.is_some())>
                            <details class="workflow-origin">
                                <summary>"查看原始业务需求（背景参考，以当前模型和本次方案为准）"</summary>
                                <p>{move || decision_workflow.get().and_then(|workflow| workflow.original_requirement_text).unwrap_or_default()}</p>
                            </details>
                        </Show>
                        <Show when=move || decision_workflow.get().is_some() && !workflow_current.get()>
                            <div class="workflow-stale-notice" role="status">"业务需求或模型基线已变化。下方为上次分析结果，请重新分析并确认新方案后生成模型；当前草稿不会被直接覆盖。"</div>
                        </Show>
                        <Show when=move || decision_workflow.get().is_none()>
                            <div class=move || if workflow_analyzing.get() { "requirement-analysis-page is-analyzing" } else { "requirement-analysis-page" } aria-busy=move || workflow_busy.get()>
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
                                        <button class="analysis-example" on:click=move |_| set_prompt.set(loan_approval_process.to_string()) disabled=move || workflow_busy.get()>
                                            <svg viewBox="0 0 1024 1024" version="1.1" xmlns="http://www.w3.org/2000/svg" p-id="6078" width="200" height="200"><path d="M891.43296 912.97792l-567.296 54.70208-191.56992-164.12672 567.296-54.70208z" fill="#F0F7F4" p-id="6079"></path><path d="M2.41664 636.06784l0.04096-46.36672c0 1.26976 0.69632 2.58048 2.19136 3.85024l-0.04096 46.36672c-1.47456-1.26976-2.19136-2.56-2.19136-3.85024" fill="#CBF7E6" p-id="6080"></path><path d="M993.21856 782.49984l0.04096-46.36672c2.6624-0.26624 5.16096-0.57344 7.49568-0.96256l-0.04096 46.36672a132.75136 132.75136 0 0 1-7.49568 0.96256" fill="#EBF7F1" p-id="6081"></path><path d="M1000.71424 781.5168l0.04096-46.36672c1.86368-0.3072 3.64544-0.65536 5.28384-1.04448l-0.04096 46.36672c-1.6384 0.38912-3.39968 0.73728-5.28384 1.04448M1005.99808 780.47232l0.04096-46.36672c1.20832-0.28672 2.3552-0.57344 3.44064-0.88064l-0.04096 46.36672c-1.08544 0.3072-2.23232 0.6144-3.44064 0.88064" fill="#EBF7F1" p-id="6082"></path><path d="M1009.43872 779.59168l0.04096-46.36672a50.9952 50.9952 0 0 0 2.47808-0.77824l-0.04096 46.36672a50.9952 50.9952 0 0 1-2.47808 0.77824" fill="#EBF7F1" p-id="6083"></path><path d="M1011.9168 778.81344l0.04096-46.36672a34.6112 34.6112 0 0 0 1.92512-0.7168l-0.04096 46.36672-1.92512 0.7168" fill="#EBF7F1" p-id="6084"></path><path d="M1013.82144 778.09664l0.04096-46.36672 1.55648-0.67584-0.04096 46.36672a39.19872 39.19872 0 0 1-1.55648 0.67584" fill="#EBF7F1" p-id="6085"></path><path d="M1015.37792 777.40032l0.04096-46.36672 1.31072-0.65536-0.04096 46.36672a15.62624 15.62624 0 0 1-1.31072 0.65536" fill="#EBF7F1" p-id="6086"></path><path d="M1016.68864 776.74496l0.04096-46.36672a14.31552 14.31552 0 0 0 1.10592-0.65536l-0.04096 46.36672a14.31552 14.31552 0 0 1-1.10592 0.65536" fill="#EBF7F1" p-id="6087"></path><path d="M1017.79456 776.0896l0.04096-46.36672a10.91584 10.91584 0 0 0 0.94208-0.65536l-0.04096 46.36672a8.45824 8.45824 0 0 1-0.94208 0.65536M1018.75712 775.43424l0.04096-46.36672 0.79872-0.65536-0.04096 46.36672a8.51968 8.51968 0 0 1-0.79872 0.65536M1019.55584 774.79936l0.04096-46.36672c0.24576-0.22528 0.47104-0.43008 0.65536-0.65536l-0.04096 46.36672-0.65536 0.65536" fill="#EBF7F1" p-id="6088"></path><path d="M1020.2112 774.12352l0.04096-46.36672a5.12 5.12 0 0 0 0.53248-0.67584l-0.04096 46.36672c-0.16384 0.22528-0.32768 0.45056-0.53248 0.67584" fill="#EBF7F1" p-id="6089"></path><path d="M1020.74368 773.44768l0.04096-46.36672c0.16384-0.22528 0.28672-0.47104 0.4096-0.69632l-0.04096 46.36672a4.77184 4.77184 0 0 1-0.4096 0.69632" fill="#EBF7F1" p-id="6090"></path><path d="M1021.15328 772.75136l0.04096-46.36672a3.56352 3.56352 0 0 0 0.26624-0.73728l-0.04096 46.36672a3.56352 3.56352 0 0 1-0.26624 0.73728M1021.41952 772.03456l0.04096-46.36672a5.12 5.12 0 0 0 0.12288-0.77824l-0.04096 46.36672a5.55008 5.55008 0 0 1-0.12288 0.77824" fill="#EBF7F1" p-id="6091"></path><path d="M1021.5424 771.23584l0.04096-46.36672v-0.12288l-0.04096 46.3872v0.1024" fill="#EBF7F1" p-id="6092"></path><path d="M4.608 639.91808l0.06144-46.36672 236.2368 202.42432-0.04096 46.36672z" fill="#CBF7E6" p-id="6093"></path><path d="M293.62176 849.96096l0.04096-46.3872 699.5968-67.46112-0.04096 46.3872z" fill="#EBF7F1" p-id="6094"></path><path d="M240.86528 842.3424l0.04096-46.36672c1.86368 1.59744 4.73088 2.99008 8.35584 4.17792l-0.04096 46.36672a24.90368 24.90368 0 0 1-8.35584-4.17792M249.22112 846.52032l0.04096-46.36672c0.94208 0.3072 1.9456 0.59392 2.9696 0.88064l-0.04096 46.36672a74.48576 74.48576 0 0 1-2.9696-0.88064M252.2112 847.40096l0.04096-46.36672c0.83968 0.22528 1.72032 0.43008 2.62144 0.63488l-0.04096 46.36672-2.62144-0.63488M254.83264 848.03584l0.04096-46.36672 2.43712 0.512-0.04096 46.36672-2.43712-0.512M257.26976 848.54784l0.04096-46.36672 2.33472 0.4096-0.04096 46.36672a68.73088 68.73088 0 0 1-2.33472-0.4096M259.60448 848.95744l0.04096-46.36672 2.29376 0.34816-0.04096 46.36672a67.97312 67.97312 0 0 1-2.29376-0.34816" fill="#EBF7F1" p-id="6095"></path><path d="M261.87776 849.3056l0.04096-46.36672 2.23232 0.3072-0.04096 46.36672-2.23232-0.3072M264.13056 849.6128l0.04096-46.36672c0.73728 0.08192 1.47456 0.18432 2.23232 0.24576l-0.04096 46.36672-2.23232-0.24576M266.36288 849.85856l0.04096-46.36672 2.2528 0.2048-0.04096 46.36672a39.99744 39.99744 0 0 1-2.2528-0.2048M268.61568 850.08384l0.04096-46.36672 2.2528 0.18432-0.04096 46.36672-2.2528-0.18432" fill="#EBF7F1" p-id="6096"></path><path d="M270.86848 850.24768l0.04096-46.36672 2.2528 0.14336-0.04096 46.36672a83.10784 83.10784 0 0 1-2.2528-0.14336M273.12128 850.39104l0.04096-46.36672 2.2528 0.1024-0.04096 46.36672-2.2528-0.1024" fill="#EBF7F1" p-id="6097"></path><path d="M275.37408 850.49344l0.04096-46.36672 2.2528 0.06144-0.04096 46.36672-2.2528-0.06144M277.62688 850.55488l0.04096-46.36672 2.23232 0.02048-0.04096 46.36672-2.23232-0.02048" fill="#EBF7F1" p-id="6098"></path><path d="M279.8592 850.57536l0.04096-46.36672h2.29376l-0.04096 46.36672h-2.29376" fill="#EBF7F1" p-id="6099"></path><path d="M282.15296 850.57536l0.04096-46.36672 2.39616-0.04096-0.04096 46.36672-2.39616 0.04096" fill="#EBF7F1" p-id="6100"></path><path d="M284.54912 850.51392l0.04096-46.36672 2.6624-0.1024-0.04096 46.36672a115.63008 115.63008 0 0 1-2.6624 0.1024" fill="#EBF7F1" p-id="6101"></path><path d="M287.21152 850.41152l0.04096-46.36672c1.3312-0.06144 2.64192-0.14336 3.95264-0.24576l-0.04096 46.36672a127.8976 127.8976 0 0 1-3.95264 0.24576M291.16416 850.16576l0.04096-46.36672 2.43712-0.2048-0.04096 46.36672-2.43712 0.2048" fill="#EBF7F1" p-id="6102"></path><path d="M993.25952 736.11264l-699.5968 67.46112c-21.79072 2.10944-45.40416-1.31072-52.75648-7.59808L4.66944 593.55136c-7.35232-6.28736 4.32128-13.1072 26.112-15.21664l699.5968-67.46112c21.77024-2.08896 45.40416 1.31072 52.75648 7.59808L1019.392 720.896c7.35232 6.30784-4.36224 13.12768-26.13248 15.21664z" fill="#DFF7EF" p-id="6103"></path><path d="M824.832 682.84416l-467.02592 40.8576-157.67552-122.5728 467.00544-40.8576z" fill="#5CE092" p-id="6104"></path><path d="M741.49888 566.74304l-50.56512-27.50464a8.76544 8.76544 0 0 0 2.39616-0.7168l50.56512 27.50464a8.54016 8.54016 0 0 1-2.39616 0.7168M743.91552 566.00576l-50.56512-27.50464a7.80288 7.80288 0 0 0 1.3312-0.77824l50.56512 27.50464a8.04864 8.04864 0 0 1-1.3312 0.77824M745.24672 565.22752l-50.56512-27.50464a12.24704 12.24704 0 0 0 1.00352-0.79872l50.56512 27.50464a7.14752 7.14752 0 0 1-1.00352 0.79872" fill="#ABFFD9" p-id="6105"></path><path d="M746.25024 564.44928l-50.56512-27.50464a11.14112 11.14112 0 0 0 0.83968-0.86016l50.56512 27.50464a11.14112 11.14112 0 0 1-0.83968 0.86016" fill="#ABFFD9" p-id="6106"></path><path d="M747.08992 563.58912l-50.56512-27.50464a10.6496 10.6496 0 0 0 0.73728-0.94208l50.56512 27.50464a8.31488 8.31488 0 0 1-0.73728 0.94208" fill="#ABFFD9" p-id="6107"></path><path d="M747.8272 562.64704l-50.56512-27.50464a11.96032 11.96032 0 0 0 0.67584-1.08544l50.56512 27.50464a9.66656 9.66656 0 0 1-0.67584 1.08544" fill="#ABFFD9" p-id="6108"></path><path d="M748.50304 561.5616l-50.56512-27.50464a9.07264 9.07264 0 0 0 0.6144-1.37216l50.56512 27.50464a8.25344 8.25344 0 0 1-0.6144 1.37216M749.11744 560.18944L698.5728 532.6848a9.95328 9.95328 0 0 0 0.63488-3.46112 7.68 7.68 0 0 0-3.80928-6.9632l50.56512 27.50464a7.65952 7.65952 0 0 1 3.80928 6.9632 10.48576 10.48576 0 0 1-0.65536 3.46112" fill="#ABFFD9" p-id="6109"></path><path d="M741.53984 548.98688l-50.56512-27.50464a7.29088 7.29088 0 0 1 2.84672 0.14336l50.56512 27.50464a7.74144 7.74144 0 0 0-2.84672-0.14336" fill="#ABFFD9" p-id="6110"></path><path d="M744.38656 549.15072l-50.56512-27.50464a7.7824 7.7824 0 0 1 1.31072 0.49152l50.56512 27.50464a5.36576 5.36576 0 0 0-1.31072-0.49152M745.6768 549.62176l-50.56512-27.50464 0.24576 0.12288 50.56512 27.50464-0.24576-0.12288" fill="#ABFFD9" p-id="6111"></path><path d="M686.77632 612.16768l-50.56512-27.50464 0.24576 0.12288 50.56512 27.50464-0.24576-0.12288" fill="#ABFFD9" p-id="6112"></path><path d="M687.02208 612.29056l-50.56512-27.50464a5.7344 5.7344 0 0 0 1.29024 0.47104l50.56512 27.50464a5.7344 5.7344 0 0 1-1.29024-0.47104" fill="#ABFFD9" p-id="6113"></path><path d="M688.31232 612.78208l-50.56512-27.50464c0.88064 0.22528 1.8432 0.28672 2.82624 0.14336a8.27392 8.27392 0 0 0 2.41664-0.73728l50.56512 27.50464a8.27392 8.27392 0 0 1-2.41664 0.73728 7.20896 7.20896 0 0 1-2.82624-0.14336M693.57568 612.18816l-50.56512-27.50464a7.80288 7.80288 0 0 0 1.3312-0.77824l50.56512 27.50464a8.04864 8.04864 0 0 1-1.3312 0.77824" fill="#ABFFD9" p-id="6114"></path><path d="M694.90688 611.4304l-50.56512-27.50464a9.1136 9.1136 0 0 0 0.98304-0.77824l50.56512 27.50464a12.63616 12.63616 0 0 1-0.98304 0.77824M695.88992 610.63168l-50.56512-27.50464a16.24064 16.24064 0 0 0 0.83968-0.83968l50.56512 27.50464a8.13056 8.13056 0 0 1-0.83968 0.83968M696.7296 609.792l-50.56512-27.50464a8.31488 8.31488 0 0 0 0.73728-0.94208l50.56512 27.50464a8.31488 8.31488 0 0 1-0.73728 0.94208" fill="#ABFFD9" p-id="6115"></path><path d="M697.46688 608.84992l-50.56512-27.50464a11.96032 11.96032 0 0 0 0.67584-1.08544l50.56512 27.50464a9.66656 9.66656 0 0 1-0.67584 1.08544" fill="#ABFFD9" p-id="6116"></path><path d="M698.14272 607.76448l-50.56512-27.50464a9.07264 9.07264 0 0 0 0.6144-1.37216l50.56512 27.50464a8.25344 8.25344 0 0 1-0.6144 1.37216" fill="#ABFFD9" p-id="6117"></path><path d="M698.75712 606.39232l-50.56512-27.50464a10.1376 10.1376 0 0 0 0.63488-3.52256l50.56512 27.50464c0 1.2288-0.22528 2.41664-0.63488 3.52256" fill="#ABFFD9" p-id="6118"></path><path d="M730.54208 533.77024l-50.56512-27.50464a9.1136 9.1136 0 0 0 2.41664-0.7168l50.56512 27.50464a8.88832 8.88832 0 0 1-2.41664 0.7168M732.95872 533.03296l-50.56512-27.50464a8.68352 8.68352 0 0 0 1.3312-0.77824l50.56512 27.50464a7.29088 7.29088 0 0 1-1.3312 0.77824" fill="#ABFFD9" p-id="6119"></path><path d="M734.28992 532.2752l-50.56512-27.50464 1.00352-0.77824 50.56512 27.50464a18.71872 18.71872 0 0 1-1.00352 0.77824M735.29344 531.47648l-50.56512-27.50464a16.24064 16.24064 0 0 0 0.83968-0.83968l50.56512 27.50464a10.83392 10.83392 0 0 1-0.83968 0.83968M736.13312 530.6368l-50.56512-27.50464a10.6496 10.6496 0 0 0 0.73728-0.94208l50.56512 27.50464a10.6496 10.6496 0 0 1-0.73728 0.94208" fill="#ABFFD9" p-id="6120"></path><path d="M736.8704 529.69472l-50.56512-27.50464a11.96032 11.96032 0 0 0 0.67584-1.08544l50.56512 27.50464a11.96032 11.96032 0 0 1-0.67584 1.08544" fill="#ABFFD9" p-id="6121"></path><path d="M737.54624 528.60928l-50.56512-27.50464a8.45824 8.45824 0 0 0 0.6144-1.39264l50.56512 27.50464a11.9808 11.9808 0 0 1-0.6144 1.39264M738.16064 527.21664L687.616 499.712a10.09664 10.09664 0 0 0 0.63488-3.50208 7.72096 7.72096 0 0 0-3.80928-6.98368l50.56512 27.50464a7.72096 7.72096 0 0 1 3.80928 6.98368 10.60864 10.60864 0 0 1-0.65536 3.50208" fill="#ABFFD9" p-id="6122"></path><path d="M686.77632 612.16768l-50.56512-27.50464a7.72096 7.72096 0 0 1-3.80928-6.98368l50.56512 27.50464c0 3.1744 1.51552 5.7344 3.80928 6.98368" fill="#ABFFD9" p-id="6123"></path><path d="M730.58304 515.9936l-50.56512-27.50464a7.43424 7.43424 0 0 1 2.8672 0.14336l50.56512 27.50464a7.43424 7.43424 0 0 0-2.8672-0.14336" fill="#ABFFD9" p-id="6124"></path><path d="M733.45024 516.13696l-50.56512-27.50464a8.78592 8.78592 0 0 1 1.29024 0.47104l50.56512 27.50464a8.78592 8.78592 0 0 0-1.29024-0.47104" fill="#ABFFD9" p-id="6125"></path><path d="M734.74048 516.62848l-50.56512-27.50464 0.24576 0.12288 50.56512 27.50464-0.24576-0.12288" fill="#ABFFD9" p-id="6126"></path><path d="M699.392 602.86976l-50.54464-27.48416 0.06144-30.24896 50.54464 27.50464z" fill="#FFFFFF" p-id="6127"></path><path d="M699.45344 572.64128l-50.54464-27.50464 42.04544-5.89824 50.54464 27.50464z" fill="#ABFFD9" p-id="6128"></path><path d="M760.9344 548.27008c4.97664-0.69632 9.0112 3.072 9.0112 8.47872 0 5.36576-4.07552 10.28096-9.05216 10.97728l-46.10048 6.47168 0.04096-19.456 46.10048-6.47168zM699.45344 572.64128l-50.54464-27.50464 0.02048-17.73568 50.56512 27.50464z" fill="#FFFFFF" p-id="6129"></path><path d="M699.4944 554.9056l-50.56512-27.50464 42.04544-5.89824 50.56512 27.48416z" fill="#ABFFD9" p-id="6130"></path><path d="M744.98048 483.14368l-50.56512-27.50464a3.9936 3.9936 0 0 0 0.49152-0.53248l50.56512 27.50464-0.49152 0.53248" fill="#ABFFD9" p-id="6131"></path><path d="M745.472 482.6112l-50.56512-27.50464a10.6496 10.6496 0 0 0 0.73728-0.94208l50.56512 27.50464a10.6496 10.6496 0 0 1-0.73728 0.94208M746.20928 481.66912l-50.56512-27.50464a11.96032 11.96032 0 0 0 0.67584-1.08544l50.56512 27.50464a11.96032 11.96032 0 0 1-0.67584 1.08544" fill="#ABFFD9" p-id="6132"></path><path d="M746.88512 480.58368L696.32 453.07904a9.29792 9.29792 0 0 0 0.6144-1.39264l50.56512 27.50464a9.29792 9.29792 0 0 1-0.6144 1.39264" fill="#ABFFD9" p-id="6133"></path><path d="M747.49952 479.19104l-50.56512-27.50464a8.97024 8.97024 0 0 0-1.1264-8.72448 6.656 6.656 0 0 0-2.06848-1.7408l50.56512 27.50464a6.9632 6.9632 0 0 1 2.06848 1.7408 8.9088 8.9088 0 0 1 1.1264 8.72448" fill="#ABFFD9" p-id="6134"></path><path d="M682.96704 605.184l-50.56512-27.50464 0.06144-30.22848 50.56512 27.50464z" fill="#FFFFFF" p-id="6135"></path><path d="M739.92192 467.98848l-50.56512-27.50464a7.4752 7.4752 0 0 1 2.8672 0.14336l50.56512 27.50464a7.10656 7.10656 0 0 0-2.8672-0.14336" fill="#ABFFD9" p-id="6136"></path><path d="M742.78912 468.13184L692.224 440.6272a6.92224 6.92224 0 0 1 1.29024 0.47104l50.56512 27.50464a6.92224 6.92224 0 0 0-1.29024-0.47104" fill="#ABFFD9" p-id="6137"></path><path d="M744.07936 468.60288l-50.56512-27.50464 0.24576 0.12288 50.56512 27.50464-0.24576-0.12288" fill="#ABFFD9" p-id="6138"></path><path d="M699.4944 554.9056l-50.56512-27.50464 0.04096-16.77312 50.54464 27.50464z" fill="#FFFFFF" p-id="6139"></path><path d="M699.51488 538.13248l-50.54464-27.50464 31.0272-4.36224 50.54464 27.50464zM708.46464 519.10656l-50.56512-27.50464 22.13888-3.11296 50.54464 27.50464z" fill="#ABFFD9" p-id="6140"></path><path d="M708.46464 519.10656l-50.56512-27.50464 36.51584-35.96288 50.56512 27.50464z" fill="#FFFFFF" p-id="6141"></path><path d="M734.84288 470.56896l-50.56512-27.50464 0.3072-0.28672 50.56512 27.50464-0.3072 0.28672" fill="#ABFFD9" p-id="6142"></path><path d="M735.15008 470.28224l-50.56512-27.50464a12.24704 12.24704 0 0 1 1.00352-0.79872l50.56512 27.50464a7.14752 7.14752 0 0 0-1.00352 0.79872" fill="#ABFFD9" p-id="6143"></path><path d="M736.1536 469.48352l-50.56512-27.50464a9.78944 9.78944 0 0 1 1.3312-0.77824l50.56512 27.50464a9.78944 9.78944 0 0 0-1.3312 0.77824" fill="#ABFFD9" p-id="6144"></path><path d="M737.4848 468.70528l-50.56512-27.50464a9.09312 9.09312 0 0 1 2.41664-0.73728l50.56512 27.50464a9.09312 9.09312 0 0 0-2.41664 0.73728" fill="#ABFFD9" p-id="6145"></path><path d="M683.02848 574.95552l-50.56512-27.50464 0.04096-17.73568 50.54464 27.50464z" fill="#ABFFD9" p-id="6146"></path><path d="M699.51488 538.13248l-50.54464-27.50464 0.02048-13.45536 50.54464 27.50464z" fill="#FFFFFF" p-id="6147"></path><path d="M698.51136 520.4992l-50.54464-27.50464 9.9328-1.39264 50.56512 27.50464z" fill="#ABFFD9" p-id="6148"></path><path d="M699.53536 524.67712l-50.56512-27.50464a8.64256 8.64256 0 0 0-1.024-4.17792l50.56512 27.50464a8.35584 8.35584 0 0 1 1.024 4.17792" fill="#FFFFFF" p-id="6149"></path><path d="M748.93312 512.08192c4.99712-0.69632 9.0112 3.072 9.0112 8.47872 0 5.36576-4.03456 10.30144-9.05216 11.01824l-34.01728 4.77184 0.02048-14.7456c0-1.72032-0.4096-3.2768-1.1264-4.56704l10.89536-1.536 24.2688-3.42016z" fill="#FFFFFF" p-id="6150"></path><path d="M698.51136 520.4992l-50.56512-27.50464a7.18848 7.18848 0 0 0-2.78528-2.80576l50.56512 27.50464a6.94272 6.94272 0 0 1 2.78528 2.80576" fill="#FFFFFF" p-id="6151"></path><path d="M683.04896 557.21984l-50.54464-27.50464 0.02048-16.77312 50.56512 27.48416z" fill="#ABFFD9" p-id="6152"></path><path d="M691.32288 516.93568l-50.56512-27.50464a7.29088 7.29088 0 0 1 2.84672 0.14336l50.56512 27.50464a7.29088 7.29088 0 0 0-2.84672-0.14336M694.1696 517.07904l-50.56512-27.50464a8.78592 8.78592 0 0 1 1.29024 0.47104l50.56512 27.50464a7.33184 7.33184 0 0 0-1.29024-0.47104" fill="#ABFFD9" p-id="6153"></path><path d="M695.48032 517.57056l-50.56512-27.50464 0.24576 0.12288 50.56512 27.50464-0.24576-0.12288" fill="#ABFFD9" p-id="6154"></path><path d="M690.688 514.06848l-50.56512-27.50464 44.17536-43.49952 50.54464 27.50464z" fill="#FFFFFF" p-id="6155"></path><path d="M690.05312 514.68288l-50.54464-27.50464 0.6144-0.6144 50.56512 27.50464z" fill="#ABFFD9" p-id="6156"></path><path d="M683.08992 540.42624l-50.56512-27.48416 0.02048-13.45536 50.56512 27.48416z" fill="#FFFFFF" p-id="6157"></path><path d="M683.1104 526.97088l-50.56512-27.50464c0-1.20832 0.22528-2.39616 0.63488-3.50208l50.56512 27.50464a10.28096 10.28096 0 0 0-0.63488 3.50208" fill="#FFFFFF" p-id="6158"></path><path d="M683.74528 523.48928l-50.56512-27.50464a13.57824 13.57824 0 0 1 0.4096-0.96256l0.22528-0.43008 50.56512 27.50464-0.22528 0.43008a13.57824 13.57824 0 0 0-0.4096 0.96256" fill="#FFFFFF" p-id="6159"></path><path d="M684.38016 522.09664l-50.56512-27.50464a11.96032 11.96032 0 0 1 0.67584-1.08544l50.56512 27.50464a8.11008 8.11008 0 0 0-0.67584 1.08544M685.03552 520.99072l-50.56512-27.50464a10.6496 10.6496 0 0 1 0.73728-0.94208l50.56512 27.50464-0.73728 0.94208M685.79328 520.04864l-50.56512-27.50464c0.26624-0.3072 0.55296-0.57344 0.83968-0.83968l50.56512 27.50464c-0.3072 0.26624-0.57344 0.55296-0.83968 0.83968M686.63296 519.20896l-50.56512-27.50464a7.31136 7.31136 0 0 1 1.00352-0.77824l50.56512 27.50464a6.2464 6.2464 0 0 0-1.00352 0.77824" fill="#FFFFFF" p-id="6160"></path><path d="M687.616 518.43072l-50.56512-27.50464a9.4208 9.4208 0 0 1 1.31072-0.75776l50.56512 27.50464a7.74144 7.74144 0 0 0-1.31072 0.75776M688.9472 517.65248l-50.56512-27.50464a8.76544 8.76544 0 0 1 2.39616-0.7168l50.56512 27.50464a8.76544 8.76544 0 0 0-2.39616 0.7168" fill="#FFFFFF" p-id="6161"></path><path d="M671.6416 524.288l-50.56512-27.50464 12.51328-1.76128 50.56512 27.50464z" fill="#FFFFFF" p-id="6162"></path><path d="M640.14336 580.97664l-50.56512-27.50464 42.88512-6.02112 50.56512 27.50464z" fill="#ABFFD9" p-id="6163"></path><path d="M635.74016 580.21888l-50.56512-27.50464 0.24576 0.12288 50.56512 27.50464-0.24576-0.12288" fill="#ABFFD9" p-id="6164"></path><path d="M635.98592 580.34176l-50.56512-27.50464c0.4096 0.2048 0.86016 0.36864 1.31072 0.49152l50.56512 27.50464a7.18848 7.18848 0 0 1-1.31072-0.49152" fill="#ABFFD9" p-id="6165"></path><path d="M637.29664 580.83328l-50.56512-27.50464c0.88064 0.22528 1.8432 0.28672 2.84672 0.14336l50.56512 27.50464a7.29088 7.29088 0 0 1-2.84672-0.14336" fill="#ABFFD9" p-id="6166"></path><path d="M640.18432 563.24096l-50.56512-27.50464 42.88512-6.02112 50.54464 27.50464z" fill="#ABFFD9" p-id="6167"></path><path d="M649.44128 545.15712l-50.54464-27.50464 33.62816-4.7104 50.56512 27.48416z" fill="#FFFFFF" p-id="6168"></path><path d="M635.74016 580.21888l-50.56512-27.50464a7.68 7.68 0 0 1-3.80928-6.94272l50.56512 27.50464c0 3.15392 1.51552 5.69344 3.80928 6.94272" fill="#ABFFD9" p-id="6169"></path><path d="M645.07904 544.41984L594.5344 516.9152l0.24576 0.12288 50.56512 27.50464a3.33824 3.33824 0 0 1-0.26624-0.12288" fill="#ABFFD9" p-id="6170"></path><path d="M645.3248 544.54272l-50.56512-27.50464a5.7344 5.7344 0 0 0 1.29024 0.47104l50.56512 27.50464a6.10304 6.10304 0 0 1-1.29024-0.47104M646.63552 545.01376l-50.56512-27.50464a7.18848 7.18848 0 0 0 2.82624 0.14336l50.56512 27.50464a7.168 7.168 0 0 1-2.82624-0.14336" fill="#ABFFD9" p-id="6171"></path><path d="M631.93088 573.27616c0-4.9152 3.72736-9.40032 8.25344-10.0352l42.88512-6.02112-0.04096 17.73568-42.88512 6.02112c-4.52608 0.63488-8.23296-2.80576-8.21248-7.70048z" fill="#FFFFFF" p-id="6172"></path><path d="M631.93088 573.27616l-50.56512-27.50464c0-1.20832 0.22528-2.39616 0.63488-3.50208l50.56512 27.50464a10.17856 10.17856 0 0 0-0.63488 3.50208" fill="#ABFFD9" p-id="6173"></path><path d="M632.56576 569.79456l-50.56512-27.50464a16.896 16.896 0 0 1 0.6144-1.39264l50.56512 27.50464a10.62912 10.62912 0 0 0-0.6144 1.39264" fill="#ABFFD9" p-id="6174"></path><path d="M633.18016 568.40192l-50.56512-27.50464a11.96032 11.96032 0 0 1 0.67584-1.08544l50.56512 27.50464a11.96032 11.96032 0 0 0-0.67584 1.08544" fill="#ABFFD9" p-id="6175"></path><path d="M633.856 567.31648l-50.56512-27.50464a10.6496 10.6496 0 0 1 0.73728-0.94208l50.56512 27.50464a10.6496 10.6496 0 0 0-0.73728 0.94208" fill="#ABFFD9" p-id="6176"></path><path d="M634.59328 566.3744l-50.56512-27.50464a8.13056 8.13056 0 0 1 0.83968-0.83968l50.56512 27.50464a10.83392 10.83392 0 0 0-0.83968 0.83968" fill="#ABFFD9" p-id="6177"></path><path d="M635.43296 565.53472l-50.56512-27.50464a9.54368 9.54368 0 0 1 1.00352-0.79872l50.56512 27.50464a12.24704 12.24704 0 0 0-1.00352 0.79872" fill="#ABFFD9" p-id="6178"></path><path d="M636.43648 564.736l-50.56512-27.50464a12.51328 12.51328 0 0 1 1.3312-0.77824l50.56512 27.50464a10.77248 10.77248 0 0 0-1.3312 0.77824" fill="#ABFFD9" p-id="6179"></path><path d="M637.76768 563.95776l-50.56512-27.50464a8.2944 8.2944 0 0 1 2.39616-0.7168l50.56512 27.50464a8.97024 8.97024 0 0 0-2.39616 0.7168" fill="#ABFFD9" p-id="6180"></path><path d="M671.6416 524.288l-50.56512-27.50464-36.39296-25.72288 50.54464 27.50464z" fill="#FFFFFF" p-id="6181"></path><path d="M649.48224 527.40096l-50.56512-27.50464 22.15936-3.11296 50.56512 27.50464z" fill="#FFFFFF" p-id="6182"></path><path d="M645.07904 544.41984L594.5344 516.9152a7.76192 7.76192 0 0 1-3.82976-6.98368c0-1.20832 0.22528-2.41664 0.63488-3.52256l50.56512 27.50464a10.1376 10.1376 0 0 0-0.63488 3.52256 7.72096 7.72096 0 0 0 3.80928 6.98368" fill="#ABFFD9" p-id="6183"></path><path d="M641.90464 533.9136l-50.56512-27.50464a16.896 16.896 0 0 1 0.6144-1.39264l50.56512 27.50464a9.29792 9.29792 0 0 0-0.6144 1.39264" fill="#ABFFD9" p-id="6184"></path><path d="M642.51904 532.52096l-50.56512-27.50464a11.96032 11.96032 0 0 1 0.67584-1.08544l50.56512 27.50464a11.96032 11.96032 0 0 0-0.67584 1.08544" fill="#ABFFD9" p-id="6185"></path><path d="M643.19488 531.43552l-50.56512-27.50464a10.6496 10.6496 0 0 1 0.73728-0.94208l50.56512 27.50464a10.6496 10.6496 0 0 0-0.73728 0.94208" fill="#ABFFD9" p-id="6186"></path><path d="M643.93216 530.51392l-50.56512-27.50464a8.13056 8.13056 0 0 1 0.83968-0.83968l50.56512 27.50464a6.51264 6.51264 0 0 0-0.83968 0.83968" fill="#ABFFD9" p-id="6187"></path><path d="M644.77184 529.67424l-50.56512-27.50464a8.6016 8.6016 0 0 1 0.98304-0.77824l50.56512 27.50464a7.12704 7.12704 0 0 0-0.98304 0.77824M645.75488 528.87552l-50.56512-27.50464a8.56064 8.56064 0 0 1 1.3312-0.75776l50.56512 27.50464a8.56064 8.56064 0 0 0-1.3312 0.75776M647.08608 528.11776l-50.56512-27.50464a8.76544 8.76544 0 0 1 2.39616-0.7168l50.56512 27.50464a7.53664 7.53664 0 0 0-2.39616 0.7168" fill="#ABFFD9" p-id="6188"></path><path d="M656.87552 527.40096l22.15936-3.11296 12.51328-1.76128a10.36288 10.36288 0 0 0-1.04448 4.46464l-0.02048 13.45536-33.64864 4.73088c-4.52608 0.63488-8.192-2.84672-8.192-7.74144 0.02048-4.93568 3.70688-9.40032 8.23296-10.0352z" fill="#FFFFFF" p-id="6189"></path><path d="M690.688 514.06848l-50.56512-27.50464-44.05248-31.10912 50.56512 27.50464z" fill="#ABFFD9" p-id="6190"></path><path d="M646.63552 482.95936l-50.56512-27.50464-0.67584-0.4096 50.56512 27.50464a11.8784 11.8784 0 0 1 0.67584 0.4096" fill="#ABFFD9" p-id="6191"></path><path d="M646.63552 482.95936l-50.56512-27.50464-0.67584-0.4096 50.56512 27.50464a11.8784 11.8784 0 0 1 0.67584 0.4096" fill="#ABFFD9" p-id="6192"></path><path d="M690.05312 514.68288l-50.54464-27.50464-44.68736-31.5392 50.56512 27.50464z" fill="#ABFFD9" p-id="6193"></path><path d="M645.38624 483.14368l-50.56512-27.50464c-0.22528-0.14336-0.43008-0.28672-0.67584-0.4096l50.56512 27.50464a4.56704 4.56704 0 0 1 0.67584 0.4096" fill="#ABFFD9" p-id="6194"></path><path d="M641.59744 481.792l-50.56512-27.50464a7.3728 7.3728 0 0 1 2.84672 0.14336l50.56512 27.50464a7.80288 7.80288 0 0 0-2.84672-0.14336" fill="#ABFFD9" p-id="6195"></path><path d="M644.44416 481.95584l-50.56512-27.50464a8.192 8.192 0 0 1 1.29024 0.47104l50.56512 27.50464a6.5536 6.5536 0 0 0-1.29024-0.47104" fill="#ABFFD9" p-id="6196"></path><path d="M645.71392 482.42688l-50.56512-27.50464 0.24576 0.12288 50.56512 27.50464-0.24576-0.12288" fill="#ABFFD9" p-id="6197"></path><path d="M641.59744 481.792l-50.56512-27.50464a7.3728 7.3728 0 0 1 2.84672 0.14336l50.56512 27.50464a7.80288 7.80288 0 0 0-2.84672-0.14336" fill="#ABFFD9" p-id="6198"></path><path d="M641.59744 481.792l-50.56512-27.50464a7.3728 7.3728 0 0 1 2.84672 0.14336l50.56512 27.50464a7.80288 7.80288 0 0 0-2.84672-0.14336" fill="#ABFFD9" p-id="6199"></path><path d="M641.59744 481.792l-50.56512-27.50464a7.3728 7.3728 0 0 1 2.84672 0.14336l50.56512 27.50464a7.80288 7.80288 0 0 0-2.84672-0.14336" fill="#ABFFD9" p-id="6200"></path><path d="M635.22816 498.56512l-50.56512-27.50464c-2.9696-2.10944-3.8912-6.26688-2.51904-10.05568l50.56512 27.50464c-1.37216 3.76832-0.45056 7.94624 2.51904 10.05568" fill="#ABFFD9" p-id="6201"></path><path d="M632.70912 488.50944l-50.56512-27.50464a9.07264 9.07264 0 0 1 0.6144-1.37216l50.56512 27.50464a16.46592 16.46592 0 0 0-0.6144 1.37216" fill="#ABFFD9" p-id="6202"></path><path d="M633.32352 487.13728l-50.56512-27.50464c0.14336-0.28672 0.32768-0.57344 0.49152-0.83968l0.16384-0.24576 50.56512 27.50464a30.5152 30.5152 0 0 0-0.65536 1.08544M633.99936 486.05184l-50.56512-27.50464a8.31488 8.31488 0 0 1 0.73728-0.94208l50.56512 27.50464a18.90304 18.90304 0 0 0-0.73728 0.94208" fill="#ABFFD9" p-id="6203"></path><path d="M634.73664 485.10976l-50.56512-27.50464c0.26624-0.3072 0.55296-0.57344 0.83968-0.83968l50.56512 27.50464a10.83392 10.83392 0 0 0-0.83968 0.83968" fill="#ABFFD9" p-id="6204"></path><path d="M635.57632 484.27008l-50.56512-27.50464a8.8064 8.8064 0 0 1 1.00352-0.77824l50.56512 27.50464a8.8064 8.8064 0 0 0-1.00352 0.77824M636.57984 483.47136l-50.56512-27.50464a9.78944 9.78944 0 0 1 1.3312-0.77824l50.56512 27.50464a12.51328 12.51328 0 0 0-1.3312 0.77824" fill="#ABFFD9" p-id="6205"></path><path d="M637.91104 482.7136l-50.56512-27.50464a9.0112 9.0112 0 0 1 2.43712-0.73728l0.63488-0.06144a7.24992 7.24992 0 0 1 2.21184 0.2048l50.56512 27.50464a7.24992 7.24992 0 0 0-2.21184-0.2048l-0.63488 0.06144a8.33536 8.33536 0 0 0-2.43712 0.73728" fill="#ABFFD9" p-id="6206"></path><path d="M643.19488 482.11968l-50.56512-27.50464a8.192 8.192 0 0 1 1.29024 0.47104l50.56512 27.50464a8.192 8.192 0 0 0-1.29024-0.47104" fill="#ABFFD9" p-id="6207"></path><path d="M644.46464 482.59072L593.92 455.08608l0.24576 0.12288 50.56512 27.50464-0.26624-0.12288" fill="#ABFFD9" p-id="6208"></path><path d="M641.59744 481.792l-50.56512-27.50464a7.3728 7.3728 0 0 1 2.84672 0.14336l50.56512 27.50464a7.80288 7.80288 0 0 0-2.84672-0.14336" fill="#ABFFD9" p-id="6209"></path><path d="M640.98304 481.91488l-50.56512-27.50464a6.41024 6.41024 0 0 1 0.6144-0.1024l50.56512 27.50464-0.6144 0.1024" fill="#ABFFD9" p-id="6210"></path><path d="M641.59744 481.792a7.02464 7.02464 0 0 1 5.03808 1.16736l44.05248 31.08864-0.6144 0.6144-44.66688-31.5392a6.94272 6.94272 0 0 0-4.4032-1.2288 7.24992 7.24992 0 0 0 0.59392-0.1024z" fill="#ABFFD9" p-id="6211"></path><path d="M759.15264 459.44832a7.5776 7.5776 0 0 1 7.10656 2.74432c3.072 3.76832 2.37568 9.99424-1.55648 13.88544l-40.0384 39.424-10.89536 1.536a7.61856 7.61856 0 0 0-7.8848-3.91168 10.6496 10.6496 0 0 0-7.86432 6.12352l-13.7216 1.92512-39.91552-28.20096c-3.91168-2.78528-4.58752-8.8064-1.536-13.45536a10.50624 10.50624 0 0 1 7.82336-4.79232 7.4752 7.4752 0 0 1 4.83328 1.35168l48.96768 34.57024 0.67584-0.67584 48.41472-47.67744a10.1376 10.1376 0 0 1 5.59104-2.84672z" fill="#FFFFFF" p-id="6212"></path><path d="M646.00064 656.09728l-55.41888-30.14656c-32.19456-17.5104-53.61664-54.00576-53.53472-99.08224 0.04096-17.46944 3.2768-34.54976 9.13408-50.46272l55.41888 30.14656a146.82112 146.82112 0 0 0-9.13408 50.46272c-0.06144 45.07648 21.34016 81.57184 53.53472 99.08224M601.62048 506.55232l-55.41888-30.14656c2.49856-6.77888 5.44768-13.35296 8.84736-19.6608l55.41888 30.14656c-3.39968 6.30784-6.3488 12.88192-8.84736 19.6608" fill="#ABFFD9" p-id="6213"></path><path d="M610.46784 486.89152l-55.41888-30.14656c2.8672-5.3248 6.0416-10.46528 9.48224-15.36l55.41888 30.14656c-3.46112 4.89472-6.61504 10.0352-9.48224 15.36" fill="#ABFFD9" p-id="6214"></path><path d="M619.9296 471.53152l-55.41888-30.14656c3.19488-4.56704 6.63552-8.92928 10.30144-13.06624l55.41888 30.14656a149.74976 149.74976 0 0 0-10.30144 13.06624M630.23104 458.4448l-55.41888-30.14656c3.64544-4.13696 7.51616-8.04864 11.5712-11.69408l55.41888 30.14656a145.03936 145.03936 0 0 0-11.5712 11.69408" fill="#ABFFD9" p-id="6215"></path><path d="M641.80224 446.7712l-55.41888-30.14656a133.3248 133.3248 0 0 1 13.59872-10.752l55.41888 30.14656a130.56 130.56 0 0 0-13.59872 10.752M655.40096 435.99872l-55.41888-30.14656c5.75488-3.97312 11.776-7.45472 18.04288-10.40384l55.41888 30.14656c-6.26688 2.94912-12.288 6.43072-18.04288 10.40384M673.44384 425.59488l-55.41888-30.14656a111.88224 111.88224 0 0 1 32.17408-9.66656l55.41888 30.14656a113.17248 113.17248 0 0 0-32.17408 9.66656" fill="#ABFFD9" p-id="6216"></path><path d="M705.61792 415.92832c62.38208-8.76544 112.82432 40.18176 112.70144 109.34272-0.12288 69.12-50.76992 132.3008-113.17248 141.06624-62.3616 8.76544-112.80384-40.20224-112.66048-109.32224 0.12288-69.16096 50.76992-132.32128 113.13152-141.08672z m55.27552 151.77728c4.97664-0.69632 9.03168-5.61152 9.05216-10.97728 0-5.38624-4.03456-9.17504-9.0112-8.47872l-46.10048 6.47168 0.04096-18.39104 34.01728-4.77184c4.99712-0.69632 9.03168-5.65248 9.05216-11.01824 0-5.38624-4.01408-9.17504-9.0112-8.47872l-24.2688 3.39968 40.0384-39.424c3.93216-3.87072 4.62848-10.11712 1.55648-13.88544a7.5776 7.5776 0 0 0-7.10656-2.74432 9.89184 9.89184 0 0 0-5.55008 2.84672l-48.41472 47.67744-48.29184-34.0992a7.5776 7.5776 0 0 0-5.5296-1.26976l-0.65536 0.12288-0.69632 0.06144a10.42432 10.42432 0 0 0-7.12704 4.73088c-3.05152 4.62848-2.39616 10.67008 1.536 13.45536l39.91552 28.20096-24.28928 3.42016c-4.97664 0.69632-8.99072 5.61152-9.0112 10.99776 0 5.36576 4.01408 9.17504 8.97024 8.47872l36.88448-5.18144-0.04096 18.39104-47.0016 6.61504c-4.97664 0.69632-9.03168 5.61152-9.05216 11.01824 0 5.36576 4.03456 9.13408 9.0112 8.43776l47.0016-6.61504-0.06144 33.13664c0 5.38624 4.01408 9.17504 8.97024 8.47872 4.99712-0.69632 9.03168-5.61152 9.05216-11.01824l0.06144-33.13664 46.05952-6.4512" fill="#2AD870" p-id="6217"></path><path d="M705.88416 513.1264a7.5776 7.5776 0 0 1 7.8848 3.91168 9.4208 9.4208 0 0 1 1.1264 4.56704l-0.02048 14.7456-0.04096 18.39104-0.04096 19.43552-0.06144 33.13664c0 5.38624-4.03456 10.30144-9.05216 11.01824-4.97664 0.69632-8.99072-3.072-8.97024-8.47872l0.06144-33.13664 0.04096-19.43552 0.04096-18.39104 0.02048-14.7456c0-1.72032 0.4096-3.3792 1.14688-4.89472a10.69056 10.69056 0 0 1 7.86432-6.12352z" fill="#FFFFFF" p-id="6218"></path><path d="M822.86592 226.44736L772.3008 198.94272l0.57344 0.26624 50.56512 27.50464-0.57344-0.26624" fill="#ABFFD9" p-id="6219"></path><path d="M823.43936 226.7136l-50.56512-27.50464a14.7456 14.7456 0 0 1 0.79872 0.4096l50.56512 27.50464-0.79872-0.4096" fill="#ABFFD9" p-id="6220"></path><path d="M705.61792 415.92832l-55.41888-30.14656a97.6896 97.6896 0 0 1 38.15424 1.96608l55.41888 30.14656a97.6896 97.6896 0 0 0-38.15424-1.96608" fill="#ABFFD9" p-id="6221"></path><path d="M743.77216 417.8944l-55.41888-30.14656c6.12352 1.57696 12.00128 3.76832 17.59232 6.51264l55.41888 30.14656a91.40224 91.40224 0 0 0-17.59232-6.51264M761.36448 424.40704L705.9456 394.24c1.14688 0.57344 2.29376 1.16736 3.42016 1.78176l55.41888 30.14656a67.23584 67.23584 0 0 0-3.42016-1.76128" fill="#ABFFD9" p-id="6222"></path><path d="M712.45824 301.99808l-50.56512-27.50464c0.67584 0.28672 1.3312 0.59392 1.98656 0.9216l50.56512 27.50464a43.02848 43.02848 0 0 0-1.98656-0.9216" fill="#ABFFD9" p-id="6223"></path><path d="M714.4448 302.91968l-50.56512-27.50464 1.57696 0.8192 50.56512 27.50464a38.11328 38.11328 0 0 0-1.57696-0.8192" fill="#ABFFD9" p-id="6224"></path><path d="M766.95552 201.19552l-50.54464-27.50464 55.88992 25.25184 50.56512 27.50464z" fill="#ABFFD9" p-id="6225"></path><path d="M715.79648 178.0736l-50.56512-27.50464 0.12288-62.01344 50.54464 27.50464z" fill="#ABFFD9" p-id="6226"></path><path d="M715.89888 116.06016l-50.56512-27.50464c0-3.97312 0.73728-7.84384 2.06848-11.44832l50.56512 27.50464c-1.31072 3.60448-2.048 7.49568-2.06848 11.44832M717.98784 104.61184l-50.56512-27.50464c0.57344-1.536 1.2288-3.03104 2.00704-4.46464l50.56512 27.50464c-0.77824 1.4336-1.45408 2.92864-2.00704 4.46464" fill="#ABFFD9" p-id="6227"></path><path d="M719.99488 100.1472l-50.56512-27.50464c0.65536-1.20832 1.37216-2.37568 2.1504-3.4816L722.1248 96.6656a31.62112 31.62112 0 0 0-2.12992 3.4816" fill="#ABFFD9" p-id="6228"></path><path d="M722.1248 96.6656l-50.56512-27.50464c0.7168-1.04448 1.51552-2.02752 2.33472-2.9696l50.56512 27.50464a37.2736 37.2736 0 0 0-2.33472 2.9696M724.45952 93.696l-50.56512-27.50464c0.8192-0.94208 1.69984-1.82272 2.62144-2.64192l50.56512 27.50464c-0.90112 0.8192-1.78176 1.69984-2.62144 2.64192M727.10144 91.0336l-50.56512-27.50464c0.98304-0.88064 2.00704-1.69984 3.09248-2.43712l50.56512 27.50464a32.3584 32.3584 0 0 0-3.09248 2.43712" fill="#ABFFD9" p-id="6229"></path><path d="M730.17344 88.59648l-50.56512-27.50464c1.31072-0.90112 2.6624-1.69984 4.096-2.3552l50.56512 27.50464a30.24896 30.24896 0 0 0-4.096 2.3552" fill="#ABFFD9" p-id="6230"></path><path d="M734.26944 86.24128l-50.56512-27.50464c2.31424-1.08544 4.77184-1.8432 7.31136-2.19136 3.03104-0.43008 5.95968-0.24576 8.66304 0.45056l50.56512 27.50464a22.20032 22.20032 0 0 0-8.66304-0.45056 25.33376 25.33376 0 0 0-7.31136 2.19136" fill="#ABFFD9" p-id="6231"></path><path d="M750.22336 84.50048l-50.56512-27.50464c1.39264 0.34816 2.72384 0.86016 3.9936 1.47456l50.56512 27.50464a19.456 19.456 0 0 0-3.9936-1.47456" fill="#ABFFD9" p-id="6232"></path><path d="M754.21696 85.97504l-50.56512-27.50464 0.77824 0.4096 50.56512 27.50464-0.77824-0.4096" fill="#ABFFD9" p-id="6233"></path><path d="M565.248 235.49952l-50.54464-27.50464 154.58304 66.49856 50.56512 27.50464z" fill="#ABFFD9" p-id="6234"></path><path d="M357.80608 673.60768l-50.56512-27.50464c-15.09376-8.21248-25.14944-25.33376-25.10848-46.46912l50.56512 27.50464c-0.04096 21.13536 9.99424 38.25664 25.10848 46.46912" fill="#ABFFD9" p-id="6235"></path><path d="M541.06112 232.38656l-50.56512-27.50464c6.06208-0.86016 12.10368-0.55296 17.87904 0.94208l50.56512 27.50464a45.58848 45.58848 0 0 0-17.87904-0.94208" fill="#ABFFD9" p-id="6236"></path><path d="M558.94016 233.32864l-50.56512-27.50464c2.1504 0.55296 4.25984 1.26976 6.30784 2.17088l50.56512 27.50464a45.8752 45.8752 0 0 0-6.30784-2.17088" fill="#ABFFD9" p-id="6237"></path><path d="M516.83328 242.31936l-50.56512-27.50464 0.69632-0.49152 50.56512 27.50464-0.69632 0.49152M517.5296 241.82784l-50.56512-27.50464c2.72384-1.88416 5.55008-3.52256 8.41728-4.87424l50.56512 27.50464c-2.8672 1.35168-5.69344 2.99008-8.41728 4.87424" fill="#ABFFD9" p-id="6238"></path><path d="M525.94688 236.9536l-50.56512-27.50464c4.93568-2.31424 10.01472-3.85024 15.11424-4.56704l50.56512 27.50464c-5.07904 0.7168-10.17856 2.2528-15.11424 4.56704" fill="#ABFFD9" p-id="6239"></path><path d="M545.97632 101.31456L495.4112 73.80992l169.82016 76.75904 50.56512 27.50464z" fill="#ABFFD9" p-id="6240"></path><path d="M533.9136 99.65568l-50.56512-27.50464c2.92864-0.4096 5.85728-0.26624 8.66304 0.47104l50.56512 27.50464a21.97504 21.97504 0 0 0-8.66304-0.47104M542.57664 100.10624l-50.56512-27.50464c1.14688 0.3072 2.29376 0.69632 3.39968 1.18784l50.56512 27.50464a24.73984 24.73984 0 0 0-3.39968-1.18784" fill="#ABFFD9" p-id="6241"></path><path d="M521.85088 104.71424L471.28576 77.2096l0.65536-0.47104 50.56512 27.50464a7.59808 7.59808 0 0 0-0.65536 0.47104" fill="#ABFFD9" p-id="6242"></path><path d="M522.50624 104.2432l-50.56512-27.50464c1.3312-0.9216 2.70336-1.69984 4.096-2.37568l50.56512 27.50464a28.672 28.672 0 0 0-4.096 2.37568" fill="#ABFFD9" p-id="6243"></path><path d="M526.60224 101.86752l-50.56512-27.50464c2.39616-1.1264 4.85376-1.86368 7.31136-2.21184l50.56512 27.50464c-2.4576 0.34816-4.93568 1.08544-7.31136 2.21184" fill="#ABFFD9" p-id="6244"></path><path d="M332.6976 627.13856l-50.54464-27.50464 0.38912-218.56256 50.56512 27.48416z" fill="#ABFFD9" p-id="6245"></path><path d="M361.92256 352.29696l-50.54464-27.50464 154.91072-109.9776 50.54464 27.50464z" fill="#ABFFD9" p-id="6246"></path><path d="M541.06112 232.38656a44.87168 44.87168 0 0 1 24.18688 3.11296l154.58304 66.49856c17.63328 7.5776 28.672 26.19392 28.63104 48.18944l-0.06144 31.1296a124.928 124.928 0 0 0-49.84832-2.7648c-80.0768 11.24352-145.12128 92.38528-145.28512 181.16608-0.06144 35.61472 10.36288 67.01056 27.9552 91.19744l-195.70688 27.50464c-29.26592 4.11648-52.89984-18.86208-52.8384-51.26144l0.4096-218.58304c0.04096-21.99552 11.1616-43.70432 28.81536-56.25856l154.91072-109.99808a55.25504 55.25504 0 0 1 24.24832-9.9328z" fill="#2AD870" p-id="6247"></path><path d="M333.1072 408.55552l-50.56512-27.50464c0.02048-8.06912 1.51552-16.09728 4.3008-23.67488l50.56512 27.50464c-2.78528 7.5776-4.28032 15.60576-4.3008 23.67488" fill="#ABFFD9" p-id="6248"></path><path d="M337.408 384.90112l-50.56512-27.50464a69.8368 69.8368 0 0 1 4.17792-9.27744l50.56512 27.50464a68.64896 68.64896 0 0 0-4.17792 9.27744" fill="#ABFFD9" p-id="6249"></path><path d="M341.56544 375.62368l-50.56512-27.50464c1.3312-2.47808 2.80576-4.87424 4.42368-7.18848l50.56512 27.50464c-1.59744 2.29376-3.072 4.68992-4.42368 7.18848M346.0096 368.4352l-50.56512-27.50464c1.49504-2.10944 3.09248-4.15744 4.79232-6.08256l50.56512 27.50464a62.5664 62.5664 0 0 0-4.79232 6.08256" fill="#ABFFD9" p-id="6250"></path><path d="M350.80192 362.35264L300.2368 334.848a67.584 67.584 0 0 1 5.38624-5.44768l50.56512 27.50464a61.46048 61.46048 0 0 0-5.38624 5.44768M356.18816 356.90496l-50.56512-27.50464c1.82272-1.6384 3.74784-3.1744 5.75488-4.608l50.56512 27.50464c-2.02752 1.4336-3.93216 2.9696-5.75488 4.608" fill="#ABFFD9" p-id="6251"></path><path d="M244.34688 307.75296l-50.56512-27.50464 277.504-203.03872 50.56512 27.50464z" fill="#ABFFD9" p-id="6252"></path><path d="M242.85184 357.19168L192.3072 329.68704a22.30272 22.30272 0 0 1-9.13408-9.50272c-3.7888-7.35232-3.85024-16.32256-0.9216-24.3712l50.56512 27.50464c-2.94912 8.04864-2.8672 17.01888 0.9216 24.3712 2.19136 4.28032 5.38624 7.4752 9.1136 9.50272M232.81664 323.29728l-50.56512-27.50464a38.7072 38.7072 0 0 1 2.02752-4.52608l50.56512 27.50464a38.7072 38.7072 0 0 0-2.02752 4.52608" fill="#ABFFD9" p-id="6253"></path><path d="M234.84416 318.79168l-50.56512-27.50464c0.63488-1.18784 1.35168-2.3552 2.12992-3.4816l50.56512 27.50464a33.66912 33.66912 0 0 0-2.12992 3.4816" fill="#ABFFD9" p-id="6254"></path><path d="M236.97408 315.31008l-50.56512-27.50464c0.7168-1.024 1.49504-2.00704 2.31424-2.94912l50.56512 27.50464c-0.8192 0.94208-1.59744 1.92512-2.31424 2.94912M239.3088 312.36096l-50.56512-27.50464a35.4304 35.4304 0 0 1 2.62144-2.64192l50.56512 27.50464a31.88736 31.88736 0 0 0-2.62144 2.64192M241.93024 309.71904L191.36512 282.2144c0.77824-0.69632 1.57696-1.35168 2.41664-1.96608l50.56512 27.50464c-0.83968 0.6144-1.65888 1.26976-2.41664 1.96608" fill="#ABFFD9" p-id="6255"></path><path d="M533.9136 99.65568a21.9136 21.9136 0 0 1 12.06272 1.65888l169.82016 76.75904 0.12288-62.01344c0.02048-15.68768 11.50976-30.02368 25.66144-32.01024 14.15168-1.98656 25.6 9.1136 25.55904 24.82176l-0.16384 92.34432 55.9104 25.25184c12.45184 5.67296 17.16224 22.20032 10.46528 36.9664-4.62848 10.24-13.49632 16.93696-22.65088 18.2272-4.096 0.57344-8.192 0.06144-12.04224-1.65888l-69.50912-31.41632-195.33824-88.2688-265.4208 194.19136a26.76736 26.76736 0 0 1-12.02176 5.03808 21.58592 21.58592 0 0 1-22.6304-11.85792c-6.63552-12.9024-1.86368-30.76096 10.60864-39.936l277.504-203.03872c3.76832-2.78528 7.92576-4.48512 12.06272-5.05856z" fill="#2AD870" p-id="6256"></path><path d="M451.33824 403.57888c10.28096-1.4336 18.65728 6.4512 18.6368 17.53088l-0.06144 37.10976c-0.02048 11.07968-8.43776 21.34016-18.71872 22.77376l-34.38592 4.83328c-10.28096 1.4336-18.65728-6.4512-18.6368-17.53088l0.06144-37.10976c0.02048-11.07968 8.43776-21.34016 18.71872-22.77376l34.38592-4.83328z" fill="#FFFFFF" p-id="6257"></path></svg>
                                            <div class="analysis-example-desc">
                                                <strong>"贷款审批"</strong><small>"信用评分、收入、逾期次数 → 审批结果"</small>
                                            </div>
                                        </button>
                                        <button class="analysis-example" on:click=move |_| set_prompt.set(order_discount_calculation.to_string()) disabled=move || workflow_busy.get()>
                                            <svg viewBox="0 0 1186 1024" version="1.1" xmlns="http://www.w3.org/2000/svg" p-id="7697" width="200" height="200"><path d="M882.756758 42.739907l0.050688 0.648814-1.601759 42.101941a54.804501 54.804501 0 0 0 36.191652 53.5677 54.814639 54.814639 0 0 1 36.222064 51.195473l-0.081101 4.744452a54.804501 54.804501 0 0 0 34.001904 50.32363l2.230298 0.861706 5.362853 1.936304a54.814639 54.814639 0 0 1 36.222064 51.195473l-0.04055 2.372226-0.212893 5.677122a54.774088 54.774088 0 0 0 33.961354 52.695856l2.230298 0.861706 5.119547 1.855202a54.794363 54.794363 0 0 1 36.222065 51.195474l-0.040551 2.362088-0.202754 5.423679a54.784226 54.784226 0 0 0 33.961354 52.705993l2.230297 0.861706 39.800679 14.415834c1.885615 0.689365 2.280986 3.000764 0.973221 4.420045l-0.57785 0.486611-0.557574 0.375095a2.970351 2.970351 0 0 1-0.598125 0.790742l-0.415647 0.334545-75.951779 50.627761-144.330685 96.206939-396.546959 265.172278 1.338179 1.946442-157.682058 103.232377a3.071728 3.071728 0 0 1-2.331675 0.466335l0.141928 0.030413a2.889249 2.889249 0 0 1-1.115149-0.050688l-2.463466 1.632172c-1.763963 1.175975-4.004398 0.253443-4.379494-1.581484a2.838561 2.838561 0 0 1-1.926166-1.956579l-0.070964-0.760329 1.622035-42.162767c0.881981-23.012619-12.895177-44.028107-34.336449-52.76682l-2.250574-0.861706-5.180373-1.86534c-21.958296-7.877007-36.505921-28.385609-36.627574-51.246162l0.040551-2.372226 0.212892-5.433817 0.010138-0.628538a42.091803 42.091803 0 0 0-35.482011-24.431899l-2.098508-0.081102-4.17674-0.050689c-17.568664-0.202754-33.079372-11.455621-39.060625-28.081477l-0.66909-2.027544-1.226663-4.034811c-5.190511-17.041503-20.234884-28.943184-37.631208-30.027919l-2.098507-0.070965-4.359219-0.050688c-17.568664-0.202754-33.08951-11.455621-39.060626-28.081478l-0.669089-2.027543-1.28749-4.217291c-5.403403-17.741006-21.45141-29.906267-39.719577-30.109021-18.278305-0.202754-34.336449-12.368015-39.739853-30.098883-5.180374-17.041503-20.224746-28.943184-37.62107-30.027919l-2.098507-0.070964-32.369732-0.375096c-1.388867-0.020275-2.230298-1.317903-1.946442-2.595256a2.372226 2.372226 0 0 1 0.13179-1.449693l0.293994-0.506886 44.930364-60.147078 15.784426-21.461548c0.912395-1.236802 1.875478-2.402639 2.88925-3.50765L436.63636 51.732063a2.230298 2.230298 0 0 1 3.061591-0.567712 2.098508 2.098508 0 0 1 2.939938 0.517023l0.263581 0.547437 9.701796 31.528302a42.720342 42.720342 0 0 0 38.148231 30.149571l2.12892 0.081102a42.689929 42.689929 0 0 1 40.277152 30.210398 42.720342 42.720342 0 0 0 38.158369 30.15971l2.12892 0.070964 4.420045 0.050689a42.679791 42.679791 0 0 1 39.597925 28.192992l0.679227 2.027544 1.317903 4.257841c0.44606 1.469969 0.973221 2.909525 1.571346 4.298392L801.878047 92.323484a2.939938 2.939938 0 0 0-1.723412 0.162203l-0.567712 0.304132 75.95178-50.627761a3.010902 3.010902 0 0 1 2.098508-0.506886l0.770466-0.496748c1.753825-1.165838 3.984123-0.253443 4.359219 1.581483z" fill="#DEDEDE" p-id="7698"></path><path d="M2.052685 598.327379l33.04896 0.375095a42.983922 42.983922 0 0 1 40.581283 30.220536 42.963647 42.963647 0 0 0 40.571145 30.230674 42.963647 42.963647 0 0 1 40.561008 30.220536l1.317904 4.247704a42.963647 42.963647 0 0 0 40.581283 30.220535l4.460595 0.060827a42.983922 42.983922 0 0 1 40.561008 30.210398l1.257077 4.065225a42.963647 42.963647 0 0 0 40.571146 30.220536l4.257841 0.050688a42.963647 42.963647 0 0 1 40.571146 30.230674l9.772759 31.528301c0.527161 1.682861 2.727046 1.966717 3.82192 0.506886l436.144883-583.648673c1.094873-1.469969 0.192617-3.487375-1.571346-3.50765l-33.048959-0.375096a42.973785 42.973785 0 0 1-40.581283-30.220536l-1.257077-4.065225a42.983922 42.983922 0 0 0-40.571146-30.220536l-4.257841-0.050688a42.963647 42.963647 0 0 1-40.571146-30.230674l-1.317903-4.237566a42.973785 42.973785 0 0 0-40.571145-30.230673l-4.450458-0.040551a42.983922 42.983922 0 0 1-40.571146-30.230674 42.963647 42.963647 0 0 0-40.571146-30.230674 42.953509 42.953509 0 0 1-40.571145-30.220536l-9.77276-31.528301c-0.527161-1.682861-2.727046-1.966717-3.821919-0.506886L0.481339 594.819729c-1.094873 1.480107-0.202754 3.497513 1.571346 3.517787" fill="#FF9517" p-id="7699"></path><path d="M659.726972 198.62759a42.700066 42.700066 0 0 1-40.277151-30.220536l-1.317904-4.257841a42.689929 42.689929 0 0 0-40.267014-30.210399l-4.430182-0.060826a42.710204 42.710204 0 0 1-40.277152-30.220536 42.689929 42.689929 0 0 0-40.277152-30.220536 42.700066 42.700066 0 0 1-40.287289-30.230674l-9.691658-31.528301c-0.527161-1.682861-2.706771-1.966717-3.801644-0.506886L396.896508 68.084201c1.084736-1.469969 3.274483-1.186113 3.791506 0.506886l9.701796 31.528302a42.689929 42.689929 0 0 0 40.277151 30.220536 42.700066 42.700066 0 0 1 40.277152 30.210398 42.710204 42.710204 0 0 0 40.28729 30.240811l4.420044 0.050689a42.700066 42.700066 0 0 1 40.277152 30.210398l1.317903 4.257841a42.700066 42.700066 0 0 0 40.267015 30.220536l33.829563 0.385234a2.037681 2.037681 0 0 1 1.926166 1.449693l10.948735 34.366863c5.555469 17.436874 2.392501 36.495783-8.505545 51.175198L324.411827 789.453767c0.202754 0.22303 0.385233 0.476473 0.486611 0.790742l18.450646 59.964599c0.506886 1.682861 2.696633 1.966717 3.791506 0.506886l390.76846-526.735527 42.223593-56.903008c1.084736-1.480107 0.202754-3.497513-1.561208-3.517788l-62.407789-0.709641a2.402639 2.402639 0 0 1-0.881982-0.243305 2.007268 2.007268 0 0 1-1.054322-1.216526l-9.99579-32.491385a42.710204 42.710204 0 0 0-40.277151-30.220536l-4.227429-0.050688z" fill="#FE6E10" p-id="7700"></path><path d="M0.471202 595.853776c-1.074598 1.469969-0.192617 3.487375 1.530795 3.50765l32.369732 0.375096c18.278305 0.202754 34.336449 12.368015 39.739853 30.098883 5.393266 17.730868 21.441273 29.896129 39.719577 30.109021 18.268167 0.202754 34.326312 12.368015 39.729715 30.098883l1.28749 4.217291c5.403403 17.741006 21.45141 29.906267 39.729715 30.109021l4.359219 0.050688c18.278305 0.202754 34.326312 12.368015 39.719577 30.109021l1.236801 4.034812c5.403403 17.730868 21.45141 29.885991 39.729715 30.098883l4.166602 0.050689c18.268167 0.202754 34.326312 12.368015 39.719578 30.109021l9.570005 31.396511c0.506886 1.672723 2.676357 1.956579 3.750955 0.496748l60.724928-82.662948c13.229721-18.014724 9.580143-43.500946-8.150725-56.94356L117.277983 504.918449a39.658751 39.658751 0 0 0-56.071716 8.282516l-60.724928 82.652811z" fill="#4393F2" p-id="7701"></path><path d="M324.411827 788.399445a2.331675 2.331675 0 0 1 0.486611 0.811017l18.957532 60.968233a2.078232 2.078232 0 0 0 2.301261 1.459832 2.331675 2.331675 0 0 0 1.581484-0.942808L435.92672 732.996818 390.955805 699.593039 324.411827 788.399445z" fill="#2E7DCC" p-id="7702"></path><path d="M518.478154 280.347731c8.556234-11.354244 19.129873-14.43611 31.193757-5.535194l43.561772 32.177115c12.063884 8.900916 12.104435 19.8091 3.527926 31.163344-8.556234 11.364381-18.906843 14.152254-30.970727 5.2412l-43.561773-32.166978c-12.074022-8.911054-12.317327-19.525244-3.750955-30.879487m97.119334 71.724351c16.696821-22.150913 19.94089-46.50171-6.548966-66.067505l-43.561772-32.15684c-26.479718-19.565795-49.157792-9.54973-65.854613 12.591045-16.899575 22.424631-19.920615 46.50171 6.559103 66.057368l43.561773 32.166978c26.499994 19.565795 48.9449 9.833586 65.844475-12.591046m-208.836981-156.952142c-6.315798-4.845829-14.932858-1.794376-19.018358 3.86247-2.463465 3.385998-3.548201 7.826318-1.206389 12.266638l126.488302 258.63345c0.760329 1.916029 1.926166 3.24407 3.294758 4.298392 4.937068 3.801644 13.422338 3.28462 18.136377-3.223794 2.858836-3.95371 3.801644-9.377389 1.743687-13.604817l-125.667146-258.004911a14.65914 14.65914 0 0 0-3.771231-4.227428m7.532324 193.731782c-8.566371 11.354244-18.916981 14.142116-30.980865 5.231062l-43.561772-32.166978c-12.063884-8.911054-12.317327-19.525244-3.750955-30.889625 8.556234-11.354244 19.129873-14.425972 31.193756-5.525056l43.561773 32.166977c12.063884 8.921191 12.094297 19.819238 3.538063 31.173482m-97.119334-71.714214c-16.929988 22.424631-19.951028 46.491573 6.548966 66.057367l43.551634 32.166978c26.489856 19.555657 48.9449 9.833586 65.854613-12.591045 16.686683-22.140775 19.930753-46.50171-6.559103-66.067505l-43.561772-32.15684c-26.489856-19.565795-49.157792-9.54973-65.844476 12.591045m-38.259746 50.252666a12.722835 12.722835 0 0 0-17.254395 2.402639l-76.712108 98.11283a11.425208 11.425208 0 0 0 2.504016 16.473791c2.737184 1.926166 6.031942 2.635807 9.154359 2.210022 3.122417-0.435922 6.082631-2.007268 8.110174-4.612661l76.701971-98.11283a11.41507 11.41507 0 0 0-2.504017-16.473791m-11.344106 180.025588c3.122417-0.435922 6.072493-2.007268 8.110175-4.612662l76.712108-98.11283a11.425208 11.425208 0 0 0-2.514154-16.473791 12.732973 12.732973 0 0 0-17.264533 2.402639l-76.712108 98.102693a11.435345 11.435345 0 0 0 2.514154 16.483928c2.737184 1.936304 6.021804 2.635807 9.154358 2.210023m141.928046 91.239457c3.122417-0.435922 6.072493-2.007268 8.110174-4.612661l76.712108-98.11283a11.425208 11.425208 0 0 0-2.514154-16.473791 12.722835 12.722835 0 0 0-17.264532 2.402639l-76.712109 98.102692a11.435345 11.435345 0 0 0 2.514154 16.483929c2.737184 1.936304 6.031942 2.635807 9.154359 2.210022m-70.964023-40.55087c3.122417-0.435922 6.062355-2.007268 8.110174-4.612661l76.712109-98.11283a11.425208 11.425208 0 0 0-2.514154-16.473791 12.732973 12.732973 0 0 0-17.264533 2.402639l-76.712109 98.102692a11.425208 11.425208 0 0 0 2.514154 16.483929c2.727046 1.936304 6.021804 2.635807 9.154359 2.210022" fill="#FFFFFF" p-id="7703"></path><path d="M103.125729 524.210526l39.800679 14.415834a54.814639 54.814639 0 0 1 36.191652 53.557562 54.794363 54.794363 0 0 0 36.191651 53.567699 54.814639 54.814639 0 0 1 36.181514 53.557562l-0.22303 5.677122a54.794363 54.794363 0 0 0 36.191652 53.557562l5.362853 1.946441a54.794363 54.794363 0 0 1 36.191651 53.5677l-0.212892 5.423679a54.784226 54.784226 0 0 0 36.191652 53.557561l5.119547 1.855203a54.804501 54.804501 0 0 1 36.191652 53.557561l-1.622035 42.122217c-0.070964 2.240436 2.463465 3.517788 4.420045 2.210022l775.677318-518.686179c1.956579-1.307766 1.723412-4.126051-0.405508-4.906656l-39.800679-14.415834a54.804501 54.804501 0 0 1-36.191652-53.567699l0.212892-5.423679a54.784226 54.784226 0 0 0-36.191651-53.557562l-5.119548-1.855202a54.794363 54.794363 0 0 1-36.191651-53.557562l0.22303-5.677122a54.804501 54.804501 0 0 0-36.191652-53.567699l-5.362853-1.926167a54.814639 54.814639 0 0 1-36.191651-53.567699 54.794363 54.794363 0 0 0-36.181514-53.567699 54.804501 54.804501 0 0 1-36.191652-53.5677l1.611897-42.101941c0.081102-2.250573-2.453328-3.527926-4.409907-2.230298L102.720221 519.303871c-1.956579 1.317903-1.723412 4.136189 0.405508 4.896517" fill="#FFB125" p-id="7704"></path><path d="M1065.20526 321.719756a54.845052 54.845052 0 0 1-36.333579-53.618388l0.223029-5.677122a54.875465 54.875465 0 0 0-36.343717-53.628525l-5.383128-1.946442a54.845052 54.845052 0 0 1-36.343717-53.618388 54.865327 54.865327 0 0 0-36.33358-53.628526 54.845052 54.845052 0 0 1-36.343717-53.618388l1.622035-42.152629c0.081102-2.250573-2.473603-3.527926-4.430183-2.230298l-75.95178 50.637899c1.956579-1.307766 4.511284-0.030413 4.420045 2.230298l-1.611897 42.162767a54.85519 54.85519 0 0 0 36.343717 53.60825 54.85519 54.85519 0 0 1 36.33358 53.628526 54.85519 54.85519 0 0 0 36.343717 53.618388l5.383128 1.936304a54.865327 54.865327 0 0 1 36.343718 53.628526l-0.22303 5.677121a54.85519 54.85519 0 0 0 36.343717 53.628526l41.189547 14.882169c1.094873 0.385233 1.794376 1.41928 1.743687 2.574981l-1.297628 46.096201a72.281926 72.281926 0 0 1-32.268355 58.089122L402.654731 891.297278a2.605393 2.605393 0 0 1 0.253443 1.155699l-3.071728 80.189346c-0.081102 2.250573 2.463465 3.527926 4.430182 2.230298l702.979747-468.646406 75.951779-50.627761c1.966717-1.317903 1.723412-4.136189-0.405508-4.906656l-76.012606-27.473214c-0.212892-0.070964-0.405509-0.182479-0.57785-0.304132a3.750955 3.750955 0 0 1-0.405509-0.344682 2.595256 2.595256 0 0 1-0.760329-1.916029l1.662586-43.450257a54.875465 54.875465 0 0 0-36.33358-53.628526l-5.160098-1.855202z" fill="#FF901D" p-id="7705"></path><path d="M92.592641 518.756434c-1.966717 1.307766-1.73355 4.126051 0.405508 4.906655l40.246739 14.43611c22.708487 8.150725 37.489279 29.815027 36.587023 53.628526-0.912395 23.803361 13.868398 45.467663 36.587022 53.628525 22.708487 8.150725 37.489279 29.815027 36.576885 53.628526l-0.212892 5.677122c-0.922532 23.803361 13.85826 45.467663 36.576885 53.628526l5.423679 1.936304c22.708487 8.160863 37.509555 29.825165 36.587022 53.628525l-0.212892 5.433817c-0.912395 23.803361 13.868398 45.467663 36.587023 53.628526l5.180373 1.855202c22.718625 8.160863 37.499417 29.825165 36.587023 53.628526l-1.622035 42.172905c-0.091239 2.240436 2.473603 3.517788 4.450458 2.210022l111.514893-73.853272c24.300109-16.088558 30.757835-48.539392 14.425972-72.48468L277.687087 459.126379c-16.342001-23.945289-49.269307-30.311775-73.569416-14.21308l-111.514893 73.853273z" fill="#61AFF6" p-id="7706"></path><path d="M408.392679 889.016291a2.331675 2.331675 0 0 1 0.172342 0.506886c0.050689 0.202754 0.091239 0.425784 0.081101 0.648814L405.513568 970.564091a2.564843 2.564843 0 0 0 0.395371 1.480107 2.676357 2.676357 0 0 0 1.794376 1.216526 3.071728 3.071728 0 0 0 2.331675-0.476473L567.717048 869.551873 527.389208 811.107932l-118.996529 77.908359z" fill="#55A2F0" p-id="7707"></path><path d="M858.466787 361.713052c14.69969-9.630832 28.314645-8.951605 38.695667 6.123181l37.509555 54.419268c10.39116 15.074786 5.900152 27.62528-8.799538 37.266249-14.69969 9.630832-27.93955 8.708299-38.33071-6.376624l-37.509555-54.40913c-10.39116-15.074786-6.275247-27.381975 8.424443-37.022944m83.63617 121.348479c28.64919-18.795328 42.537863-45.518352 19.727998-78.597724l-37.509555-54.419268c-22.809864-33.08951-53.415634-30.64632-82.064823-11.86113-29.034423 19.028496-42.537863 45.498076-19.727999 78.587587l37.509555 54.419267c22.809864 33.099648 53.0304 30.889625 82.064824 11.86113M761.752961 209.839905c-5.72781-8.546096-17.609215-8.61706-25.009749-3.629302-4.44032 2.990627-7.653977 7.836456-6.630067 14.162391l47.039009 364.268466c0.13179 2.605393 1.013772 4.714039 2.260711 6.569241 4.470733 6.680756 15.125475 9.732209 23.641158 3.994261 5.190511-3.487375 8.61706-9.580143 7.866868-15.520846l-46.298955-363.163455a18.592574 18.592574 0 0 0-2.868975-6.690893m-79.175573 239.554265c-14.709828 9.640969-27.93955 8.718437-38.320573-6.366487l-37.509555-54.419267c-10.401298-15.064648-6.285385-27.371837 8.414306-37.012807 14.69969-9.640969 28.314645-8.951605 38.695668 6.123181l37.509555 54.419268c10.39116 15.074786 5.910289 27.62528-8.799539 37.26625m-83.626032-121.348479c-29.034423 19.038634-42.537863 45.508214-19.727998 78.597724l37.509555 54.419267c22.809864 33.08951 53.0304 30.899763 82.064823 11.86113 28.64919-18.775053 42.537863-45.498076 19.727998-78.587586l-37.509555-54.419268c-22.809864-33.08951-53.415634-30.64632-82.064823-11.861129m-64.232578 54.034034a15.064648 15.064648 0 0 0-21.18783-4.237566l-131.678813 90.458853a15.814839 15.814839 0 0 0-4.146326 21.644027c2.35195 3.568477 5.900152 5.829188 9.732209 6.599655 3.82192 0.770467 7.947971 0.040551 11.435345-2.351951l131.688951-90.468991a15.814839 15.814839 0 0 0 4.156464-21.644027M448.254185 577.636297c3.832057 0.770467 7.947971 0.040551 11.435345-2.362088l131.688951-90.458853a15.814839 15.814839 0 0 0 4.156464-21.644027 15.064648 15.064648 0 0 0-21.177692-4.237566l-131.688951 90.468991a15.804702 15.804702 0 0 0-4.146326 21.633889c2.35195 3.568477 5.890014 5.829188 9.732209 6.599654m121.65261 182.478916c3.82192 0.770467 7.947971 0.040551 11.435345-2.351951l131.688951-90.468991a15.814839 15.814839 0 0 0 4.156464-21.644027 15.064648 15.064648 0 0 0-21.177692-4.237566l-131.68895 90.458854a15.794564 15.794564 0 0 0-4.146327 21.633889c2.35195 3.578614 5.890014 5.849463 9.732209 6.609792m-60.826305-91.239458c3.82192 0.770467 7.947971 0.040551 11.435345-2.35195l131.688951-90.468992a15.814839 15.814839 0 0 0 4.156464-21.644027 15.054511 15.054511 0 0 0-21.177692-4.237565l-131.688951 90.458853a15.814839 15.814839 0 0 0-4.146326 21.644027c2.35195 3.578614 5.900152 5.839325 9.732209 6.599654" fill="#FFFFFF" p-id="7708"></path></svg>
                                            <div class="analysis-example-desc">
                                                <strong>"订单折扣"</strong><small>"订单金额、客户等级 → 折扣和规则说明"</small>
                                            </div>
                                        </button>
                                        <button class="analysis-example" on:click=move |_| set_prompt.set(flight_delay_resolution.to_string()) disabled=move || workflow_busy.get()>
                                            <svg viewBox="0 0 1024 1024" version="1.1" xmlns="http://www.w3.org/2000/svg" p-id="9950" width="200" height="200">
                                                <path d="M525.088 83.712a48.08 48.08 0 0 0-46.368 35.584L288.224 830.208a48.064 48.064 0 0 0 33.936 58.8l370.944 99.392a48.128 48.128 0 0 0 58.752-33.952L942.336 243.52a48.048 48.048 0 0 0-33.904-58.784L537.472 85.344a47.904 47.904 0 0 0-12.384-1.632z" fill="#F5F9EC" p-id="9951"></path>
                                                <path d="M871.136 509.248L625.488 263.6h-185.44l-151.824 566.608a48.064 48.064 0 0 0 33.936 58.8l370.944 99.392a48.128 48.128 0 0 0 58.752-33.952l119.28-445.2z" fill="#E4E8DB" p-id="9952"></path>
                                                <path d="M395.888 428.384l463.632 124.224 82.832-309.104a48.048 48.048 0 0 0-33.904-58.784L537.472 85.344a48.08 48.08 0 0 0-58.736 33.952l-82.848 309.088z" fill="#DCC69F" p-id="9953"></path>
                                                <path d="M395.888 428.384l463.632 124.224 11.616-43.36-245.648-245.648h-185.44z" fill="#CDB994" p-id="9954"></path>
                                                <path d="M353.072 897.28L568.4 93.632l-30.944-8.288a48.08 48.08 0 0 0-58.736 33.952L288.224 830.208a48.064 48.064 0 0 0 33.936 58.8l30.912 8.272z" fill="#EA672C" p-id="9955"></path>
                                                <path d="M96 928c-8.816 0-16-7.168-16-16V272c0-8.832 7.184-16 16-16h480c26.464 0 48 21.536 48 48v576c0 26.464-21.536 48-48 48H96z" fill="#125D9D" p-id="9956"></path>
                                                <path d="M352 336c-106.032 0-192 85.968-192 192s85.968 192 192 192 192-85.968 192-192-85.952-192-192-192z m142.096 120h-69.2c-6.624-33.168-16.96-59.968-27.504-80.64a159.744 159.744 0 0 1 96.704 80.64zM400 528c0 14.288-1.072 27.408-2.544 40h-90.912A339.616 339.616 0 0 1 304 528c0-14.288 1.056-27.408 2.512-40h90.976c1.456 12.592 2.512 25.712 2.512 40z m-88.144-72c8.88-40.096 23.536-69.232 35.488-87.536 1.584-0.048 3.056-0.464 4.656-0.464 1.616 0 3.104 0.432 4.704 0.48 12 18.416 26.608 47.568 35.456 87.52h-80.304z m-5.248-80.64c-10.512 20.64-20.864 47.472-27.488 80.64h-69.216a159.744 159.744 0 0 1 96.704-80.64z m-108.928 112.64h76.592A375.408 375.408 0 0 0 272 528c0 14.16 0.96 27.328 2.32 40h-76.64C194.336 555.136 192 541.888 192 528c0-13.888 2.336-27.136 5.68-40z m12.224 112h69.296c6.688 33.232 17.04 60.272 27.36 80.624a159.728 159.728 0 0 1-96.656-80.624z m137.44 87.536c-11.472-17.648-26.4-46.848-35.456-87.536h80.224c-9.04 40.656-23.952 69.856-35.456 87.536-1.584 0.048-3.056 0.464-4.656 0.464s-3.072-0.416-4.656-0.464z m50.112-6.912c10.336-20.384 20.672-47.392 27.344-80.624h69.296a159.68 159.68 0 0 1-96.64 80.624z m108.864-112.624h-76.624c1.344-12.672 2.304-25.84 2.304-40 0-14.144-0.928-27.328-2.256-40h76.576c3.344 12.864 5.68 26.112 5.68 40 0 13.888-2.336 27.136-5.68 40z" fill="#E7BE29" p-id="9957"></path><path d="M192 816h320v32H192z" fill="#FFFFFF" p-id="9958"></path>
                                                <path d="M718.24 647.296l16.56-61.824 30.912 8.288-16.576 61.824zM643.616 925.472l57.984-216.368 30.896 8.288-57.968 216.352z" fill="#010101" p-id="9959"></path>
                                                <path d="M912.544 169.28L541.632 69.888a64.032 64.032 0 0 0-78.384 45.264L429.792 240H96a32 32 0 0 0-32 32v640a32 32 0 0 0 32 32h369.6l223.328 59.84a64.032 64.032 0 0 0 78.384-45.264l190.496-710.928a64 64 0 0 0-45.264-78.368zM96 272h480c17.648 0 32 14.352 32 32v576c0 17.648-14.352 32-32 32H96V272z m830.896-32.624L736.4 950.304a32.032 32.032 0 0 1-39.2 22.64l-111.584-29.904C616.368 938.352 640 912.048 640 880V626.32l16.384 4.4 16.576-61.824-30.912-8.288-2.048 7.648V313.952l32.656 8.752 37.84 123.728 26.496 7.104-2.512-114.256 46.8 12.544 20.576 36.272 17.664 4.736-3.04-50.512 27.888-42.224-17.664-4.736-35.952 21.136-46.816-12.544 59.296-97.696-26.496-7.104-94.64 88.224-49.92-13.376C621.424 253.872 600.432 240 576 240h-113.072l31.232-116.576a32.08 32.08 0 0 1 39.2-22.64l370.912 99.392c8.256 2.208 15.152 7.504 19.424 14.912a31.84 31.84 0 0 1 3.2 24.288z" fill="#010101" p-id="9960"></path><path d="M718.256 531.84l8.192-30.928 30.4 8.048-8.176 30.928zM779.072 547.92l8.192-30.928 30.4 8.032-8.176 30.944zM657.44 515.76l8.192-30.928 30.4 8.048-8.176 30.928z" fill="#010101" p-id="9961"></path>
                                            </svg>
                                            <div class="analysis-example-desc">
                                                <strong>"航班服务"</strong><small>"延误时长、舱位、联程 → 服务方案"</small>
                                            </div>
                                        </button>
                                    </div>
                                </div>
                                <div class="workflow-requirement-editor analysis-editor">
                                    <div class="workflow-requirement-heading">
                                        <label for="decision-requirement">"业务需求"</label>
                                        <label class="workflow-requirement-import" title="导入会替换当前需求，支持 TXT 和 Markdown">
                                            <input
                                                class="knowledge-import-input"
                                                type="file"
                                                accept=".txt,.md,.markdown,text/plain,text/markdown"
                                                disabled=move || workflow_locked.get() || requirement_importing.get() || !can_model.get()
                                                on:change=import_requirement_document
                                            />
                                            <span>{move || if requirement_importing.get() { "读取中..." } else { "导入需求文档" }}</span>
                                        </label>
                                    </div>
                                    <textarea
                                        id="decision-requirement"
                                        class="workflow-requirement-input"
                                        placeholder="描述需要分析的业务决策需求..."
                                        prop:value=prompt
                                        on:input=move |event| set_prompt.set(event_target_value(&event))
                                        on:keydown=on_keydown
                                        disabled=move || workflow_locked.get() || requirement_importing.get() || !can_model.get()
                                    ></textarea>
                                    <Show when=move || requirement_import_error.get().is_some()>
                                        <small class="workflow-requirement-import-error" role="alert">{move || requirement_import_error.get().unwrap_or_default()}</small>
                                    </Show>
                                    <Show when=move || can_model.get()>
                                        <button
                                            class="button button-primary analysis-submit"
                                            on:click=analyze_requirement
                                            disabled=move || workflow_locked.get() || prompt.get().trim().is_empty()
                                        >
                                            {move || if workflow_analyzing.get() { "分析中..." } else { "开始分析需求" }}
                                        </button>
                                        <button
                                            class="button button-quiet"
                                            on:click=recommend_current_requirement
                                            disabled=move || pattern_recommendations_busy.get() || prompt.get().trim().is_empty()
                                        >{move || if pattern_recommendations_busy.get() { "检索中..." } else { "查找相似已发布模式" }}</button>
                                    </Show>
                                    <Show when=move || !pattern_recommendations.get().is_empty()>
                                        <section class="pattern-recommendations workflow-pattern-recommendations" aria-label="需求相关的已发布模式">
                                            <strong>"可复用的已发布模式"</strong>
                                            <For
                                                each=move || { pattern_recommendations.get().into_iter().take(3).collect::<Vec<_>>() }
                                                key=|item| item.recommendation_id
                                                let:item
                                            >
                                                {
                                                    let version_id = item.source_version_id;
                                                    let project_name = item.source_project_name.clone();
                                                    let version_number = item.source_version_number;
                                                    view! {
                                                        <article class="pattern-recommendation">
                                                            <div>
                                                                <strong>{item.name}</strong>
                                                                <span>{format!("匹配度 {:.0}%", item.match_score)}</span>
                                                            </div>
                                                            <small>{format!("来源：{} v{}", project_name, version_number)}</small>
                                                            <small>{format!("模式演进 v{}", item.evolution_number)}</small>
                                                            <p>{item.recommendation_reason}</p>
                                                            <button
                                                                class="button button-quiet"
                                                                on:click=move |_| {
                                                                    if let Some(history) = model_library.get().into_iter().find(|entry| entry.source_version_id == Some(version_id)) {
                                                                        select_model_library_item(history.source_kind, history.source_id, history.name);
                                                                    }
                                                                    set_workspace_view.set(WorkspaceView::Resources);
                                                                    set_resource_view.set(ResourceView::Models);
                                                                }
                                                            >"前往模型库复用"</button>
                                                        </article>
                                                    }
                                                }
                                            </For>
                                        </section>
                                    </Show>
                                </div>
                            </div>
                        </Show>
                        <Show when=move || decision_workflow.get().is_some()>
                            {move || decision_workflow.get().map(|workflow| view! {
                                <DecisionBriefView brief=workflow.brief question_count=if workflow.plan.is_none() { workflow.clarifications.len() } else { 0 } />
                            })}
                        </Show>
                        <Show when=move || decision_workflow.get().as_ref().is_some_and(|workflow| !workflow.clarifications.is_empty() && workflow.status != "confirmed")>
                            <section id="workflow-clarifications" class="analysis-questions" aria-label="需要你确认的问题">
                                <div class="analysis-card-heading"><h3>"需要你确认的问题"</h3><span>{move || decision_workflow.get().map(|workflow| format!("第 {} 轮 · 最多 3 项", workflow.clarifications.iter().map(|item| item.round).max().unwrap_or(1))).unwrap_or_default()}</span></div>
                                <p class="analysis-section-description">"这些回答可能影响最终判断，请按实际业务情况补充。"</p>
                                <div class="clarification-list">
                                    <For each=move || decision_workflow.get().map(|workflow| {
                                        let current_round = workflow.clarifications.iter().map(|item| item.round).max().unwrap_or(1);
                                        workflow.clarifications.into_iter().filter(|item| item.round == current_round).collect::<Vec<_>>()
                                    }).unwrap_or_default() key=|item| item.id.clone() let:item>
                                        <ClarificationInput item=item answers=clarification_answers set_answers=set_clarification_answers disabled=Signal::derive(move || workflow_locked.get() || !can_model.get()) />
                                    </For>
                                </div>
                            </section>
                        </Show>
                        <Show when=move || decision_workflow.get().and_then(|workflow| workflow.plan).is_some()>
                            {move || decision_workflow.get().and_then(|workflow| {
                                let plan = workflow.plan?;
                                Some(view! { <DecisionPlanView plan=plan requirements=workflow.brief.requirement_items /> })
                            })}
                        </Show>
                        <Show when=move || decision_workflow.get().is_some()>
                            <details class="analysis-details analysis-requirement-details" open=move || !workflow_current.get()>
                                <summary>"查看 / 调整业务需求"<span>"重新分析后将重新规划与确认，不会直接覆盖当前模型"</span></summary>
                                <div class="workflow-requirement-editor">
                                    <label for="decision-requirement">"业务需求"</label>
                                    <textarea
                                        id="decision-requirement"
                                        class="workflow-requirement-input"
                                        placeholder="描述需要分析的业务决策需求..."
                                        prop:value=prompt
                                        on:input=move |event| set_prompt.set(event_target_value(&event))
                                        on:keydown=on_keydown
                                        disabled=move || workflow_locked.get() || !can_model.get()
                                    ></textarea>
                                </div>
                            </details>
                        </Show>
                        <Show when=move || workflow_error.get().is_some() || pending_requirement_review.get().is_some()>
                            <Show when=move || workflow_error.get().is_some()>
                                <div class="workflow-error" role="alert">
                                    <span class="workflow-error-icon" aria-hidden="true">"!"</span>
                                    <div>
                                        <strong>{move || if workflow_error.get().is_some_and(|message| message.contains("决策项目")) { "需要先选择项目" } else { "需求与方案处理未完成" }}</strong>
                                        <span>{move || workflow_error.get().unwrap_or_default()}</span>
                                    </div>
                                </div>
                            </Show>
                            <Show when=move || workflow_retry_id.get().is_some()>
                                <button
                                    class="button button-quiet"
                                    on:click=retry_requirement_analysis
                                    disabled=move || workflow_locked.get() || workflow_busy.get()
                                    title="仅重试已识别的临时模型故障"
                                >
                                    "重试临时故障"
                                </button>
                            </Show>
                            <Show when=move || plan_retry_id.get().is_some()>
                                <button
                                    class="button button-quiet"
                                    on:click=retry_decision_plan
                                    disabled=move || workflow_locked.get() || workflow_busy.get()
                                    title="仅重试已识别的临时模型故障"
                                >
                                    "重试方案规划"
                                </button>
                            </Show>
                            <Show when=move || facilitator_retry_id.get().is_some()>
                                <button
                                    class="button button-quiet"
                                    on:click=retry_clarification
                                    disabled=move || workflow_locked.get() || workflow_busy.get()
                                    title="仅重试已识别的临时模型故障"
                                >
                                    "重试澄清主持"
                                </button>
                            </Show>
                            <Show when=move || pending_requirement_review.get().is_some()>
                                <section class="requirement-human-review" aria-label="需求分析人工复核">
                                    <div class="analysis-card-heading">
                                        <h3>{move || if pending_requirement_review.get().is_some_and(|review| review.purpose == "facilitator") { "选择澄清主持结果" } else { "选择需求分析结果" }}</h3>
                                        <span>"多模型结果未达成共识"</span>
                                    </div>
                                    <p class="requirement-review-requirement">{move || pending_requirement_review.get().map(|review| review.requirement_text).unwrap_or_default()}</p>
                                    <p class="workflow-action-hint">{move || if pending_requirement_review.get().is_some_and(|review| review.purpose == "facilitator") { "请比较各模型提出的新澄清问题；确认后会保留本轮已提交的回答。" } else { "请比较各模型的需求摘要，选择一份作为本项目后续方案规划的依据。所选结果仍会经过结构校验。" }}</p>
                                    <div class="requirement-review-candidates">
                                        <For
                                            each=move || pending_requirement_review.get().map(|review| review.candidates).unwrap_or_default()
                                            key=|candidate| candidate.model.clone()
                                            children=move |candidate| {
                                                let model = candidate.model.clone();
                                                let selected_model = candidate.model.clone();
                                                let brief = candidate.analysis.brief.clone();
                                                let clarifications = RwSignal::new(candidate.analysis.clarifications);
                                                let has_clarifications = !clarifications.get_untracked().is_empty();
                                                view! {
                                                    <div class="requirement-review-option">
                                                        <button
                                                            type="button"
                                                            class=move || if selected_review_model.get().as_deref() == Some(selected_model.as_str()) { "requirement-review-candidate is-selected" } else { "requirement-review-candidate" }
                                                            on:click=move |_| set_selected_review_model.set(Some(model.clone()))
                                                        >
                                                            <span class="requirement-review-model">{candidate.model}</span>
                                                            <strong>{candidate.analysis.brief.name}</strong>
                                                            <span>{candidate.analysis.brief.goal}</span>
                                                            <small>{format!("输入 {} 项 · 输出 {} 项 · 风险 {}", candidate.analysis.brief.inputs.len(), candidate.analysis.brief.outputs.len(), candidate.analysis.brief.risk_level)}</small>
                                                        </button>
                                                        <details class="requirement-review-detail">
                                                            <summary>"查看完整摘要与澄清问题"</summary>
                                                            <DecisionBriefView brief=brief />
                                                            <Show when=move || has_clarifications>
                                                                <ul class="requirement-review-clarifications">
                                                                    <For each=move || clarifications.get() key=|item| item.id.clone() let:item>
                                                                        <li>{item.question}</li>
                                                                    </For>
                                                                </ul>
                                                            </Show>
                                                        </details>
                                                    </div>
                                                }
                                            }
                                        />
                                    </div>
                                    <button
                                        class="button button-primary"
                                        on:click=confirm_human_requirement_review
                                        disabled=move || review_busy.get() || selected_review_model.get().is_none()
                                    >
                                        {move || if review_busy.get() { "正在提交复核..." } else { "采用所选结果并继续" }}
                                    </button>
                                </section>
                            </Show>
                        </Show>
                        <Show when=move || workflow_analyzing.get() || workflow_planning.get() || workflow_facilitating.get()>
                            <div id="workflow-analysis-progress">
                                <RequirementAnalysisProgress
                                    prompt=prompt
                                    agent_activity=agent_activity
                                    orchestration_stage=workflow_orchestration_stage
                                    orchestration_events=workflow_orchestration_events
                                    planning=workflow_planning
                                    on_cancel=Callback::new(move |_| stop_workflow_operation(()))
                                />
                            </div>
                        </Show>
                        <Show when=move || can_model.get()>
                        <div class="workflow-actions">
                            <Show when=move || decision_workflow.get().is_some()>
                                <button
                                    class="button button-quiet"
                                    on:click=analyze_requirement
                                    title="重新分析当前业务需求，成功后重新规划与确认方案"
                                    disabled=move || workflow_locked.get() || workflow_busy.get() || prompt.get().trim().is_empty()
                                >
                                    {move || if workflow_analyzing.get() { "分析中..." } else { "重新分析" }}
                                </button>
                            </Show>
                            <Show when=move || !workflow_analyzing.get() && !workflow_planning.get()>
                                <Show when=move || decision_workflow.get().as_ref().is_some_and(|workflow| workflow.status == "clarification" && workflow.plan.is_none())>
                                    <button
                                        class="button button-primary"
                                        on:click=continue_clarification
                                        disabled=move || workflow_locked.get() || !workflow_current.get() || (missing_clarifications.get() > 0)
                                        title="每轮最多追问 3 项；未回答的可选问题会跳过"
                                    >
                                        "提交本轮并继续澄清"
                                    </button>
                                    <Show when=move || (missing_clarifications.get() > 0)>
                                        <span class="workflow-action-hint">{move || format!("还需回答 {} 个必答问题", missing_clarifications.get())}</span>
                                    </Show>
                                </Show>
                                <Show when=move || decision_workflow.get().as_ref().is_some_and(|workflow| workflow.status != "clarification" && (workflow.plan.is_none() || (workflow.status == "planned" && workflow_answers_changed.get())) && !simple_direct_workflow_eligible(workflow))>
                                    <button class="button button-primary" on:click=create_decision_plan disabled=move || workflow_locked.get() || !workflow_current.get() || (missing_clarifications.get() > 0)>
                                        {move || if workflow_planning.get() { "规划中..." } else if workflow_answers_changed.get() { "按新回答重新规划" } else { "生成决策方案" }}
                                    </button>
                                    <Show when=move || (missing_clarifications.get() > 0)><span class="workflow-action-hint">{move || format!("还需回答 {} 个必答问题", missing_clarifications.get())}</span></Show>
                                </Show>
                                <Show when=move || decision_workflow.get().as_ref().is_some_and(simple_direct_workflow_eligible)>
                                    <div class="workflow-ready-actions">
                                        <span class="workflow-action-hint">"将直接生成候选模型；请检查结果后再确认保存为草稿"</span>
                                        <button class="button button-primary" on:click=move |_| generate_model_action() disabled=move || workflow_locked.get() || !workflow_current.get()>
                                            "直接生成模型候选"
                                        </button>
                                    </div>
                                </Show>
                                <Show when=move || decision_workflow.get().as_ref().is_some_and(|workflow| workflow.status == "planned") && !workflow_answers_changed.get()>
                                    <button class="button button-primary" on:click=confirm_decision_plan disabled=move || workflow_locked.get() || !workflow_current.get()>
                                        {move || if workflow_confirming.get() { "确认中..." } else { "确认方案" }}
                                    </button>
                                </Show>
                                <Show when=move || decision_workflow.get().as_ref().is_some_and(|workflow| workflow.status == "confirmed")>
                                    <div class="workflow-ready-actions">
                                        <span class="workflow-ready">{move || if workflow_current.get() { "方案已确认，可生成模型" } else { "旧方案不可用于当前需求，请重新分析" }}</span>
                                        <button
                                            class="button button-primary"
                                            on:click=generate_model_from_plan
                                            disabled=move || workflow_locked.get() || !workflow_current.get() || workflow_answers_changed.get()
                                        >
                                            {move || if current_version.get().is_some() { "按方案生成变更预览" } else { "使用方案生成模型" }}
                                        </button>
                                    </div>
                                </Show>
                            </Show>
                        </div>
                        </Show>
                    </section>
                </Show>
            <Show when=move || workspace_view.get() == WorkspaceView::Model>
            <section id="workspace-panel-model" class="model-pane-shell" role="tabpanel" aria-labelledby="workspace-tab-model" tabindex="0" inert=move || project_busy.get()>
                <div class="model-pane-tabs" role="group" aria-label="建模工作区面板">
                    <button
                        id="model-pane-tab-conversation"
                        class=move || if mobile_model_focus.get() == MobileModelFocus::Conversation { "model-pane-tab is-active" } else { "model-pane-tab" }
                        type="button"
                        aria-controls="workspace-pane-conversation"
                        aria-pressed=move || mobile_model_focus.get() == MobileModelFocus::Conversation
                        on:click=move |_| set_mobile_model_focus.set(MobileModelFocus::Conversation)
                    >"建模对话"<span class="model-pane-status">{move || if pending_review.get().is_some() && can_model.get() { "待确认" } else if unread_messages.get() { "有更新" } else { "" }}</span></button>
                    <button
                        id="model-pane-tab-inspector"
                        class=move || if mobile_model_focus.get() == MobileModelFocus::Inspector { "model-pane-tab is-active" } else { "model-pane-tab" }
                        type="button"
                        aria-controls="workspace-pane-inspector"
                        aria-pressed=move || mobile_model_focus.get() == MobileModelFocus::Inspector
                        on:click=move |_| set_mobile_model_focus.set(MobileModelFocus::Inspector)
                    >"模型产物"</button>
                </div>
                <div
                    class=move || format!(
                        "workspace-grid model-focus-{}{}{}",
                        mobile_model_focus.get().class(),
                        if model_expanded.get() { " model-grid-expanded" } else { "" },
                        if resizing_workspace.get() { " is-resizing" } else { "" },
                    )
                    node_ref=workspace_grid
                    style=move || if viewport_width.get() <= 1100.0 || model_expanded.get() {
                        String::new()
                    } else {
                        format!(
                            "grid-template-columns: {}px 8px minmax(0, 1fr);",
                            conversation_width.get()
                        )
                    }
                >
                <section id="workspace-pane-conversation" class="conversation-panel panel" aria-label="建模对话">
                    <div class="panel-heading conversation-heading">
                        <div class="heading-copy">
                            <p class="eyebrow">"AGENT CONVERSATION"</p>
                            <h2>{move || if !can_model.get() { "查看当前决策模型" } else if generation_mode.get() == GenerationMode::Modify { "修改当前决策模型" } else { "方案驱动建模" }}</h2>
                            <p class="heading-description">{move || if !can_model.get() { "当前角色仅可查看模型产物与验收结果。" } else if generation_mode.get() == GenerationMode::Modify { "调整业务需求后先分析变更、规划并确认方案，再生成 DSL 与 DMN 预览，确认后才保存草稿。" } else { "请在需求与方案页确认方案，再使用方案生成模型。" }}</p>
                        </div>
                    </div>
                    <div class="conversation-content">
                    <div id="conversation-history" class="conversation-scroll" node_ref=message_list tabindex="0" aria-label="建模对话与变更确认"
                        on:scroll=move |_| {
                            if viewport_width.get_untracked() <= 760.0 {
                                return;
                            }
                            if let Some(element) = message_list.get_untracked() {
                                if element.client_height() == 0 {
                                    return;
                                }
                                let near_bottom = element.scroll_height() - element.scroll_top() - element.client_height() <= 64;
                                follow_messages.set(near_bottom);
                                if near_bottom {
                                    unread_messages.set(false);
                                }
                            }
                        }
                    >
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
                    <Show when=move || pending_review.get().is_some() && can_model.get()>
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
                    <Show when=move || unread_messages.get()>
                        <button type="button" class="button button-quiet conversation-jump" aria-controls="conversation-history"
                            on:click=move |_| {
                                follow_messages.set(true);
                                unread_messages.set(false);
                                if let Some(element) = message_list.get_untracked() {
                                    if viewport_width.get_untracked() <= 760.0 {
                                        let _ = element.focus();
                                        element.scroll_into_view_with_bool(false);
                                    } else {
                                        element.set_scroll_top(element.scroll_height());
                                        let _ = element.focus();
                                    }
                                }
                            }
                        >{move || if pending_review.get().is_some() && can_model.get() { "有新变更，查看并确认 ↓" } else { "有新内容，回到底部 ↓" }}</button>
                    </Show>
                    <Show when=move || can_model.get()>
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
                            <details class="known-dsl-panel">
                                <summary>"当前版本 DSL · 受控补丁基线"<span>"只读"</span></summary>
                                <label class="sr-only" for="known-nl_dsl">"当前版本 DSL · 受控补丁基线"</label>
                                <textarea
                                    id="known-nl_dsl"
                                    class="known-nl_dsl-editor"
                                    prop:value=source_dsl
                                    readonly=true
                                ></textarea>
                            </details>
                        </Show>
                        <Show when=move || generation_mode.get() == GenerationMode::Modify>
                            <textarea
                                class="composer"
                                aria-label="业务需求与本次调整"
                                placeholder="可直接调整原始业务需求，或描述本次规则变更；将结合当前 DSL 先分析、再规划，不会直接修改模型。"
                                prop:value=prompt
                                on:input=move |event| set_prompt.set(event_target_value(&event))
                                on:keydown=on_keydown
                                disabled=move || workflow_locked.get()
                            ></textarea>
                        </Show>
                        <div class="composer-footer">
                            /*<span class="composer-hint">
                                {move || if !workflow_current.get() {
                                    "Ctrl / Cmd + Enter 分析当前需求"
                                } else if workflow_answers_changed.get() {
                                    "回答已调整，请重新规划方案"
                                } else {
                                    match decision_workflow.get().as_ref().map(|workflow| workflow.status.as_str()) {
                                        None => "Ctrl / Cmd + Enter 分析需求",
                                        Some("analyzed" | "clarification") => "Ctrl / Cmd + Enter 生成方案",
                                        Some("planned") => "Ctrl / Cmd + Enter 确认方案",
                                        _ => "Ctrl / Cmd + Enter 生成模型",
                                    }
                                }}
                            </span>*/
                            <div class="composer-actions">
                                <Show when=move || status.get() == RunStatus::Running>
                                    <button class="button button-danger" on:click=stop>"停止运行"</button>
                                </Show>
                                <Show when=move || generation_mode.get() == GenerationMode::Modify>
                                <button
                                    class="button button-quiet"
                                    on:click=move |_| set_workspace_view.set(WorkspaceView::Discovery)
                                >"查看需求与方案"</button>
                                <button
                                    class="button button-primary"
                                    on:click=analyze_requirement
                                    disabled=move || prompt.get().trim().is_empty()
                                        || workflow_locked.get()
                                        || current_project.get().is_none()
                                        || current_version.get().is_none()
                                        || source_dsl.get().trim().is_empty()
                                >{move || if workflow_analyzing.get() { "分析中..." } else { "分析需求变更" }}</button>
                                </Show>
                            </div>
                        </div>
                    </div>
                    </Show>
                    </div>
                </section>

                <div
                    class="model-workspace-divider"
                    role="separator"
                    tabindex="0"
                    aria-label="调整建模对话与模型产物宽度"
                    aria-valuemin="320"
                    aria-valuemax=move || workspace_grid
                        .get()
                        .map(|grid| {
                            ((f64::from(grid.client_width()) - 420.0).max(320.0).min(560.0))
                                .round()
                                .to_string()
                        })
                        .unwrap_or_else(|| "560".into())
                    aria-valuenow=move || conversation_width.get().round().to_string()
                    on:mousedown=resize_start
                    on:keydown=resize_keydown
                ></div>

                <aside id="workspace-pane-inspector" class="inspector-column" aria-label="模型产物与执行过程">
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
                            <button
                                type="button"
                                class="button button-quiet model-expand-button"
                                aria-controls="workspace-pane-conversation"
                                aria-expanded=move || model_expanded.get()
                                title=move || if model_expanded.get() { "恢复建模对话" } else { "展开模型产物" }
                                on:click=move |_| set_model_expanded.update(|expanded| *expanded = !*expanded)
                            >{move || if model_expanded.get() { "恢复双栏" } else { "展开" }}</button>
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
                </div>
            </section>
            </Show>
            </div>
            </Show>

            <Show when=move || show_test.get()>
                <div class="test-drawer-backdrop" on:click=move |_| set_show_test.set(false)></div>
                <aside class="test-drawer" aria-label="DMN 逻辑验证">
                    <div class="test-drawer-heading">
                        <div class="test-title-block">
                            <span class="test-title-icon">"D"</span>
                            <div><p class="eyebrow">"DMN QUALITY & EVALUATION"</p><h2>"规则逻辑验证"</h2></div>
                        </div>
                        // <button class="button button-quiet test-close" on:click=move |_| set_show_test.set(false) title="关闭核验与测试面板">"关闭"</button>
                        <button
                            class="knowledge-diagnostics-close"
                            type="button"
                            aria-label="关闭核验与测试面板"
                            title="关闭核验与测试面板"
                            autofocus
                            on:click=move |_| set_show_test.set(false)
                        >
                            <span class="knowledge-diagnostics-close-icon" aria-hidden="true">"×"</span>
                        </button>
                    </div>
                    <div class="test-body" node_ref=test_drawer_body>
                        <section class="test-workbenfch">
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
                    <Show when=move || engineer_retry_id.get().is_some()>
                        <button class="button button-quiet" on:click=retry disabled=move || status.get() == RunStatus::Running title="按原需求重新执行生成；仍受预算和重试次数限制">
                            "重试生成"
                        </button>
                    </Show>
                </div>
            </Show>
            <footer class="footer-note">"同源安全连接 · API 密钥仅保留在服务端"</footer>
        </main>
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
