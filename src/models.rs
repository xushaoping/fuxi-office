use serde::{Deserialize, Serialize};

// 后端专用：users 表行结构（仅后端查询使用；wasm 前端不需要，避免引入 sqlx）
#[derive(Debug)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
pub struct UserRecord {
    pub username: String,
    pub password_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct AdminUser {
    pub username: String,
    pub role: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUser {
    pub username: String,
    pub password: String,
    #[serde(default = "default_user_role")]
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUser {
    pub role: String,
    #[serde(default)]
    pub password: Option<String>,
}

fn default_user_role() -> String {
    "user".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct DecisionProject {
    pub id: i64,
    pub owner_username: String,
    pub name: String,
    pub description: String,
    pub risk_level: String,
    pub current_draft_version_id: Option<i64>,
    pub published_version_id: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDecisionProject {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_risk_level")]
    pub risk_level: String,
}

/// A collaborator assigned to a decision project. Project ownership remains on
/// `DecisionProject`; this list contains only delegated responsibilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct ProjectMember {
    pub project_id: i64,
    pub username: String,
    pub role: String,
    pub added_by: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProjectMember {
    pub role: String,
}

fn default_risk_level() -> String {
    "medium".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct DecisionVersion {
    pub id: i64,
    pub project_id: i64,
    pub version_number: i32,
    pub status: String,
    pub base_version_id: Option<i64>,
    #[serde(default)]
    pub rollback_source_version_id: Option<i64>,
    #[serde(default)]
    pub rollback_reason: Option<String>,
    pub change_summary: Option<serde_json::Value>,
    pub created_by: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub reviewed_by: Option<String>,
    pub reviewed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDecisionVersion {
    pub dsl_yaml: String,
    pub dmn_xml: String,
    #[serde(default)]
    pub change_summary: Option<serde_json::Value>,
    #[serde(default)]
    pub decision_brief: Option<serde_json::Value>,
    #[serde(default)]
    pub decision_plan: Option<serde_json::Value>,
    #[serde(default)]
    pub candidate_id: Option<i64>,
    #[serde(default)]
    pub knowledge_citations: Vec<KnowledgeCitation>,
    #[serde(default)]
    pub change_confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelChangeIntent {
    pub base_revision: i64,
    /// Hash of the canonical server-side base model. The LLM does not author
    /// this value; the generation endpoint fills it from the fetched artifact.
    #[serde(default)]
    pub base_hash: String,
    pub changes: Vec<ModelChangeOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelChangeOperation {
    pub target: String,
    pub action: String,
    #[serde(default)]
    pub field: Option<String>,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelChangePreview {
    pub candidate_id: i64,
    pub project_id: i64,
    pub base_revision: i64,
    /// Server-issued immutable revision token captured by this candidate.
    #[serde(default)]
    pub base_revision_id: Option<String>,
    pub base_hash: String,
    pub dsl_yaml: String,
    pub dmn_xml: String,
    pub status: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    pub change_review: Option<VersionChangeReview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackDecisionVersion {
    pub reason: String,
    #[serde(default)]
    pub rollback_confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionWorkspace {
    pub project: DecisionProject,
    pub version: Option<DecisionVersion>,
    pub dsl_yaml: Option<String>,
    pub dmn_xml: Option<String>,
    #[serde(default)]
    pub knowledge_citations: Vec<KnowledgeCitation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSource {
    pub id: i64,
    pub owner_username: String,
    pub source_type: String,
    pub name: String,
    pub description: String,
    pub content: String,
    pub version_label: String,
    pub source_uri: String,
    pub valid_from: Option<chrono::NaiveDate>,
    pub valid_until: Option<chrono::NaiveDate>,
    pub status: String,
    pub is_effective: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveKnowledgeSource {
    pub source_type: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub content: String,
    #[serde(default)]
    pub version_label: String,
    #[serde(default)]
    pub source_uri: String,
    pub valid_from: Option<chrono::NaiveDate>,
    pub valid_until: Option<chrono::NaiveDate>,
    #[serde(default = "default_knowledge_status")]
    pub status: String,
}

fn default_knowledge_status() -> String {
    "active".into()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeCitation {
    pub source_id: i64,
    pub source_type: String,
    pub name: String,
    pub version_label: String,
    pub source_uri: String,
    pub valid_from: Option<chrono::NaiveDate>,
    pub valid_until: Option<chrono::NaiveDate>,
    #[serde(default)]
    pub source_updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct AuditEvent {
    pub id: i64,
    pub project_id: i64,
    pub version_id: Option<i64>,
    pub event_type: String,
    pub actor_username: String,
    pub summary: String,
    pub details: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelLibraryItem {
    pub source_kind: String,
    pub source_id: String,
    pub name: String,
    pub description: String,
    pub industry: String,
    pub risk_level: String,
    pub source_project_id: Option<i64>,
    pub source_project_name: Option<String>,
    pub source_version_id: Option<i64>,
    pub source_version_number: Option<i32>,
    pub test_case_count: i64,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDecisionTemplate {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub industry: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstantiateModelLibraryItem {
    pub project_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub instantiate_confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTemplateTestCase {
    pub name: String,
    pub category: String,
    pub input_json: serde_json::Value,
    pub expected_output_json: serde_json::Value,
    pub source: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VersionDiffEntry {
    pub change_type: String,
    pub element_type: String,
    pub element: String,
    #[serde(default)]
    pub path: String,
    pub before: serde_json::Value,
    pub after: serde_json::Value,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VersionRiskFinding {
    pub level: String,
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub element: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VersionChangeReview {
    pub version_id: Option<i64>,
    pub base_version_number: Option<i32>,
    pub target_version_number: Option<i32>,
    pub overall_risk: String,
    pub summary: String,
    #[serde(default)]
    pub changes: Vec<VersionDiffEntry>,
    #[serde(default)]
    pub findings: Vec<VersionRiskFinding>,
    pub confirmation_required: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VerificationGate {
    pub code: String,
    pub name: String,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VerificationTestCase {
    pub name: String,
    pub category: String,
    pub input: serde_json::Value,
    pub expectation: String,
    pub status: String,
    pub actual: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VerificationFinding {
    pub level: String,
    pub code: String,
    pub message: String,
    pub element: String,
    #[serde(default)]
    pub rule_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VersionVerificationReport {
    pub version_id: i64,
    pub status: String,
    pub generated_at: chrono::DateTime<chrono::Utc>,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub boundary_cases: usize,
    #[serde(default)]
    pub gates: Vec<VerificationGate>,
    #[serde(default)]
    pub cases: Vec<VerificationTestCase>,
    #[serde(default)]
    pub findings: Vec<VerificationFinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct DecisionTestCase {
    pub id: i64,
    pub version_id: i64,
    pub name: String,
    pub category: String,
    pub input_json: serde_json::Value,
    pub expected_output_json: serde_json::Value,
    pub actual_output_json: Option<serde_json::Value>,
    #[serde(default)]
    pub execution_trace_json: Option<serde_json::Value>,
    pub status: String,
    pub source: String,
    pub approved_by: Option<String>,
    pub approved_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionTrace {
    pub model_name: String,
    pub version_id: Option<i64>,
    pub version_number: Option<i32>,
    pub status: String,
    pub normalized_input: serde_json::Value,
    pub output: serde_json::Value,
    #[serde(default)]
    pub decisions: Vec<DecisionTraceStep>,
    #[serde(default)]
    pub messages: Vec<ExecutionTraceMessage>,
    pub explanation: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionTraceStep {
    pub order: usize,
    pub decision_id: String,
    pub decision_name: String,
    pub logic_type: String,
    pub status: String,
    pub outcome: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub matched_rule_ids: Vec<String>,
    pub result: serde_json::Value,
    pub explanation: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionTraceMessage {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub decision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDecisionTestCase {
    pub name: String,
    pub input_json: serde_json::Value,
    pub expected_output_json: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveDecisionTestCase {
    pub approved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateDecisionTestCasesResult {
    pub cases: Vec<DecisionTestCase>,
    pub generated_count: usize,
    pub duplicate_count: usize,
    pub covered_rule_count: usize,
    pub total_rule_count: usize,
    pub skipped_rule_count: usize,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionServiceField {
    pub name: String,
    pub type_ref: String,
    pub source: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PublishedDecisionService {
    pub project_id: i64,
    pub version_id: i64,
    pub version_number: i32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub inputs: Vec<DecisionServiceField>,
    #[serde(default)]
    pub outputs: Vec<DecisionServiceField>,
}

/// Metadata for a credential that can invoke one published decision service.
/// The secret is never stored or returned after creation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct DecisionServiceApiKey {
    pub id: i64,
    pub project_id: i64,
    pub service_id: String,
    pub name: String,
    pub key_prefix: String,
    pub created_by: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDecisionServiceApiKey {
    pub service_id: String,
    pub name: String,
    #[serde(default)]
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedDecisionServiceApiKey {
    pub key: DecisionServiceApiKey,
    /// Returned exactly once. Store it in the caller's secret manager.
    pub secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteDecisionService {
    pub service_id: String,
    #[serde(default)]
    pub inputs: serde_json::Value,
    #[serde(default)]
    pub business_reference: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDecisionRunOutcome {
    #[serde(default)]
    pub business_reference: String,
    pub actual_outcome: serde_json::Value,
    pub assessment: String,
    #[serde(default)]
    pub note: String,
    pub occurred_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct DecisionServiceRun {
    pub id: i64,
    pub project_id: i64,
    pub version_id: i64,
    pub service_id: String,
    pub service_name: String,
    pub input_json: serde_json::Value,
    pub output_json: serde_json::Value,
    pub execution_trace_json: serde_json::Value,
    pub duration_ms: i64,
    pub status: String,
    pub executed_by: String,
    pub executed_at: chrono::DateTime<chrono::Utc>,
    pub business_reference: String,
    pub actual_outcome_json: Option<serde_json::Value>,
    pub outcome_assessment: Option<String>,
    pub outcome_note: String,
    pub outcome_occurred_at: Option<chrono::DateTime<chrono::Utc>>,
    pub outcome_reported_by: Option<String>,
    pub outcome_reported_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionAnalyticsSummary {
    pub total_runs: usize,
    pub succeeded_runs: usize,
    pub failed_runs: usize,
    pub outcome_reported_runs: usize,
    pub confirmed_runs: usize,
    pub mismatch_runs: usize,
    pub inconclusive_runs: usize,
    pub failure_rate: f64,
    pub feedback_coverage_rate: f64,
    pub mismatch_rate: f64,
    pub average_duration_ms: f64,
    pub p95_duration_ms: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionRuleHitMetric {
    pub rule_id: String,
    pub hit_count: usize,
    pub last_hit_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionRuleCoverageMetric {
    pub decision_id: String,
    pub decision_name: String,
    pub total_rules: usize,
    pub covered_rules: usize,
    pub coverage_rate: f64,
    pub execution_count: usize,
    pub default_output_count: usize,
    pub no_match_count: usize,
    #[serde(default)]
    pub rules: Vec<DecisionRuleHitMetric>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionResultValueMetric {
    pub value: String,
    pub count: usize,
    pub rate: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionResultDistributionMetric {
    pub decision_id: String,
    pub decision_name: String,
    pub total: usize,
    #[serde(default)]
    pub values: Vec<DecisionResultValueMetric>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionDailyTrendMetric {
    pub date: chrono::NaiveDate,
    pub total_runs: usize,
    pub succeeded_runs: usize,
    pub failed_runs: usize,
    pub outcome_reported_runs: usize,
    pub mismatch_runs: usize,
    pub average_duration_ms: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionAnalyticsFinding {
    pub level: String,
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub element: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionAnalyticsReport {
    pub project_id: i64,
    pub version_id: i64,
    pub version_number: i32,
    pub window_days: u32,
    pub window_start: chrono::DateTime<chrono::Utc>,
    pub generated_at: chrono::DateTime<chrono::Utc>,
    pub summary: DecisionAnalyticsSummary,
    #[serde(default)]
    pub rule_coverage: Vec<DecisionRuleCoverageMetric>,
    #[serde(default)]
    pub result_distributions: Vec<DecisionResultDistributionMetric>,
    #[serde(default)]
    pub daily_trends: Vec<DecisionDailyTrendMetric>,
    #[serde(default)]
    pub findings: Vec<DecisionAnalyticsFinding>,
}

/// A human-reviewed improvement opportunity derived from a published version's
/// deterministic runtime analysis. It is intentionally separate from a model
/// change: confirmation creates a draft for subsequent modelling and review.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct DecisionOptimizationProposal {
    pub id: i64,
    pub project_id: i64,
    pub source_version_id: i64,
    pub source_version_number: i32,
    pub finding_code: String,
    pub finding_level: String,
    pub element: String,
    pub title: String,
    pub summary: String,
    pub proposed_actions: serde_json::Value,
    pub evidence: serde_json::Value,
    pub analysis_snapshot: serde_json::Value,
    pub status: String,
    pub generated_by: String,
    pub generated_at: chrono::DateTime<chrono::Utc>,
    pub resolved_by: Option<String>,
    pub resolved_at: Option<chrono::DateTime<chrono::Utc>>,
    pub resolution_note: String,
    pub resulting_version_id: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishDecisionVersion {
    #[serde(default)]
    pub publish_confirmed: bool,
}
