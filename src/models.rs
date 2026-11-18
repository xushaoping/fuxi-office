use serde::{Deserialize, Serialize};

// 后端专用：users 表行结构（仅后端查询使用；wasm 前端不需要，避免引入 sqlx）
#[derive(Debug)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
pub struct UserRecord {
    pub username: String,
    pub password_hash: String,
    pub role: String,
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
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct PromptAssetAdminView {
    pub id: i64,
    pub prompt_key: String,
    pub version: String,
    pub content: String,
    pub content_digest: String,
    pub status: String,
    pub evaluation: Option<serde_json::Value>,
    pub created_by: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub evaluated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub activated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub retired_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct PromptAssetEventAdminView {
    pub id: i64,
    pub prompt_asset_id: i64,
    pub prompt_key: String,
    pub version: String,
    pub event_type: String,
    pub actor_username: String,
    pub details: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePromptDraftRequest {
    pub prompt_key: String,
    pub version: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptEvaluationRequest {
    pub dataset_hash: String,
    pub case_count: u32,
    pub passed: bool,
    pub report: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct PromptEvaluationJobView {
    pub id: i64,
    pub prompt_asset_id: i64,
    pub status: String,
    pub completed_cases: i32,
    pub total_cases: i32,
    pub result: Option<serde_json::Value>,
    pub error_message: Option<String>,
    pub cancel_requested: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub heartbeat_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct ModelPerformanceMetricView {
    pub model_name: String,
    pub task_type: String,
    pub sample_count: i64,
    pub success_count: i64,
    pub success_rate: f64,
    pub avg_latency_ms: i32,
    pub estimated_cost_usd: Option<f64>,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct ModelExperimentMetricView {
    pub task_type: String,
    pub experiment_name: String,
    pub variant: String,
    pub sample_count: i64,
    pub success_count: i64,
    pub success_rate: f64,
    pub success_rate_ci_lower: f64,
    pub success_rate_ci_upper: f64,
    pub minimum_sample_size_reached: bool,
    pub treatment_minus_control_ci_lower: Option<f64>,
    pub treatment_minus_control_ci_upper: Option<f64>,
    pub comparison_sample_size_reached: bool,
    pub treatment_effect_significant: bool,
    pub avg_latency_ms: i64,
    pub treatment_percent: Option<i16>,
    pub treatment_percent_config_count: i64,
    pub audit_sample_count: i64,
    pub assignment_integrity_percent: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct ModelUsageMetricView {
    pub task_type: String,
    pub run_count: i64,
    #[serde(default)]
    pub transport_attempt_count: i64,
    pub request_count: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub estimated_cost_usd: Option<f64>,
    pub estimated_cost_run_count: i64,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct DecisionExplanationAdminMetricView {
    pub model: Option<String>,
    pub prompt_version: String,
    pub total_count: i64,
    pub succeeded_count: i64,
    pub failed_count: i64,
    pub running_count: i64,
    pub latency_sample_count: i64,
    pub p50_latency_ms: Option<i64>,
    pub p95_latency_ms: Option<i64>,
    pub review_count: i64,
    pub reviewed_explanation_count: i64,
    pub readable_count: i64,
    pub needs_revision_count: i64,
    pub inconclusive_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrchestrationHistoryItem {
    pub id: i64,
    pub parent_orchestration_id: Option<i64>,
    pub retry_of_orchestration_id: Option<i64>,
    pub trigger_kind: String,
    pub status: String,
    pub current_stage: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub steps: Vec<AgentOrchestrationStepView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrchestrationStepView {
    pub id: i64,
    pub orchestration_id: i64,
    pub step_no: i32,
    pub attempt_no: i32,
    pub agent_role: String,
    pub task_kind: String,
    pub status: String,
    pub model: Option<String>,
    pub prompt_version: Option<String>,
    pub artifact_kind: Option<String>,
    pub artifact_schema_version: i16,
    pub artifact_digest: Option<String>,
    pub usage: Option<serde_json::Value>,
    pub error: Option<serde_json::Value>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[cfg(test)]
mod model_usage_tests {
    use super::{AgentOrchestrationHistoryItem, ModelUsageMetricView};

    #[test]
    fn orchestration_history_response_deserializes_flat_parent_and_step_summaries() {
        let item: AgentOrchestrationHistoryItem = serde_json::from_value(serde_json::json!({
            "id": 7,
            "parentOrchestrationId": 3,
            "retryOfOrchestrationId": null,
            "triggerKind": "workflow",
            "status": "succeeded",
            "currentStage": "tester",
            "createdAt": "2026-11-14T08:00:00Z",
            "finishedAt": "2026-11-14T08:00:05Z",
            "steps": [{
                "id": 11,
                "orchestrationId": 7,
                "stepNo": 2,
                "attemptNo": 1,
                "agentRole": "tester",
                "taskKind": "semantic_test_proposals",
                "status": "succeeded",
                "model": "test-model",
                "promptVersion": "tester-v1",
                "artifactKind": "semantic_test_proposals",
                "artifactSchemaVersion": 1,
                "artifactDigest": "0123456789abcdef",
                "usage": {"prompt_tokens": 12, "completion_tokens": 4, "total_tokens": 16},
                "error": null,
                "startedAt": "2026-11-14T08:00:02Z",
                "finishedAt": "2026-11-14T08:00:05Z"
            }]
        }))
        .expect("orchestration history shape should match the API response");

        assert_eq!(item.id, 7);
        assert_eq!(item.steps.len(), 1);
        assert_eq!(item.steps[0].usage.as_ref().unwrap()["total_tokens"], 16);
    }

    #[test]
    fn older_usage_responses_default_transport_attempt_count_to_zero() {
        let metric: ModelUsageMetricView = serde_json::from_value(serde_json::json!({
            "taskType": "dsl_generation",
            "runCount": 1,
            "requestCount": 1,
            "promptTokens": 10,
            "completionTokens": 5,
            "totalTokens": 15,
            "estimatedCostUsd": null,
            "estimatedCostRunCount": 0,
            "lastUpdated": "2026-11-06T00:00:00Z"
        }))
        .expect("older response should deserialize");

        assert_eq!(metric.transport_attempt_count, 0);
    }
}

#[cfg(test)]
mod search_diagnostic_tests {
    use super::{KnowledgeSearchDiagnostics, PatternSearchDiagnostics};

    #[test]
    fn search_diagnostic_dtos_match_existing_api_field_names() {
        let knowledge: KnowledgeSearchDiagnostics = serde_json::from_value(serde_json::json!({
            "keyword_results": [{
                "id": 1,
                "source_name": "Policy",
                "source_type": "policy",
                "content": "approved content",
                "metadata": {}
            }],
            "vector_results": [],
            "fused_results": []
        }))
        .unwrap();
        assert_eq!(knowledge.keyword_results[0].source_name, "Policy");

        let patterns: PatternSearchDiagnostics = serde_json::from_value(serde_json::json!({
            "keywordCandidateIds": [2],
            "vectorCandidates": [[3, 0.875]],
            "rrfCandidateIds": [3, 2]
        }))
        .unwrap();
        assert_eq!(patterns.vector_candidates, vec![(3, 0.875)]);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct AdminBusinessQualityMetricView {
    pub project_id: i64,
    pub project_name: String,
    pub owner_username: String,
    pub total_runs: i64,
    pub succeeded_runs: i64,
    pub conclusive_outcome_runs: i64,
    pub mismatch_runs: i64,
    pub feedback_coverage_percent: f64,
    pub mismatch_rate_percent: f64,
    pub last_run_at: chrono::DateTime<chrono::Utc>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSourceIndexStatus {
    pub source_id: i64,
    pub chunk_count: i64,
    pub embedded_chunk_count: i64,
    pub current_model_chunk_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeIndexStatus {
    pub embedding_enabled: bool,
    pub vector_available: bool,
    pub current_model: String,
    pub sources: Vec<KnowledgeSourceIndexStatus>,
}

#[cfg(test)]
mod knowledge_index_status_tests {
    use super::{KnowledgeIndexStatus, KnowledgeSourceIndexStatus};

    #[test]
    fn knowledge_index_status_uses_the_frontend_camel_case_contract() {
        let status = KnowledgeIndexStatus {
            embedding_enabled: true,
            vector_available: true,
            current_model: "model-v2".into(),
            sources: vec![KnowledgeSourceIndexStatus {
                source_id: 7,
                chunk_count: 4,
                embedded_chunk_count: 3,
                current_model_chunk_count: 2,
            }],
        };
        let json = serde_json::to_value(&status).unwrap();

        assert_eq!(json["embeddingEnabled"], true);
        assert_eq!(json["vectorAvailable"], true);
        assert_eq!(json["currentModel"], "model-v2");
        assert_eq!(json["sources"][0]["sourceId"], 7);
        assert_eq!(json["sources"][0]["chunkCount"], 4);
        assert_eq!(json["sources"][0]["embeddedChunkCount"], 3);
        assert_eq!(json["sources"][0]["currentModelChunkCount"], 2);
    }
}

#[cfg(test)]
mod knowledge_search_diagnostics_tests {
    use super::KnowledgeSearchDiagnostics;

    #[test]
    fn vector_status_is_optional_for_older_diagnostic_responses() {
        let diagnostics: KnowledgeSearchDiagnostics = serde_json::from_value(serde_json::json!({
            "keyword_results": [],
            "vector_results": [],
            "fused_results": []
        }))
        .unwrap();
        assert!(diagnostics.vector_status.is_empty());
        assert_eq!(diagnostics.minimum_vector_similarity, None);
    }

    #[test]
    fn vector_status_is_included_in_diagnostic_responses() {
        let diagnostics: KnowledgeSearchDiagnostics = serde_json::from_value(serde_json::json!({
            "keyword_results": [],
            "vector_results": [],
            "fused_results": [],
            "vector_status": "provider_error"
        }))
        .unwrap();
        assert_eq!(diagnostics.vector_status, "provider_error");
    }

    #[test]
    fn vector_diagnostics_include_scores_and_the_active_minimum() {
        let diagnostics: KnowledgeSearchDiagnostics = serde_json::from_value(serde_json::json!({
            "keyword_results": [],
            "vector_results": [{
                "id": 9,
                "source_name": "Policy",
                "source_type": "policy",
                "content": "approved content",
                "metadata": {},
                "similarity": 0.73
            }],
            "fused_results": [],
            "vector_status": "ready",
            "minimum_vector_similarity": 0.1
        }))
        .unwrap();
        assert_eq!(diagnostics.vector_results[0].similarity, Some(0.73));
        assert_eq!(diagnostics.minimum_vector_similarity, Some(0.1));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct KnowledgeSearchChunkDiagnostic {
    pub id: i64,
    pub source_name: String,
    pub source_type: String,
    pub content: String,
    pub metadata: serde_json::Value,
    #[serde(default)]
    pub similarity: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct KnowledgeSearchDiagnostics {
    pub keyword_results: Vec<KnowledgeSearchChunkDiagnostic>,
    pub vector_results: Vec<KnowledgeSearchChunkDiagnostic>,
    pub fused_results: Vec<KnowledgeSearchChunkDiagnostic>,
    #[serde(default)]
    pub vector_status: String,
    #[serde(default)]
    pub minimum_vector_similarity: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatternSearchDiagnostics {
    pub keyword_candidate_ids: Vec<i64>,
    pub vector_candidates: Vec<(i64, f64)>,
    pub rrf_candidate_ids: Vec<i64>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PatternRecommendation {
    pub recommendation_id: i64,
    pub pattern_id: i64,
    pub source_project_id: i64,
    pub source_project_name: String,
    pub source_version_id: i64,
    pub source_version_number: i32,
    pub pattern_type: String,
    pub name: String,
    pub description: String,
    pub match_score: f64,
    pub recommendation_reason: String,
    pub success_rate: Option<f64>,
    pub usage_count: i32,
    pub evolution_number: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PatternLineageItem {
    pub pattern_id: i64,
    pub source_project_id: i64,
    pub source_project_name: String,
    pub source_version_id: i64,
    pub source_version_number: i32,
    pub version_status: String,
    pub pattern_type: String,
    pub category: String,
    pub name: String,
    pub description: String,
    pub success_rate: Option<f64>,
    pub usage_count: i32,
    pub evolution_number: i32,
    pub supersedes_pattern_id: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PatternUsageMetric {
    pub pattern_id: i64,
    pub name: String,
    pub source_project_id: i64,
    pub source_version_id: i64,
    pub evolution_number: i32,
    pub usage_count: i32,
    pub success_rate: Option<f64>,
    pub last_used: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PatternLibraryAnalytics {
    pub total_patterns: i64,
    pub total_usage_count: i64,
    pub average_success_rate: Option<f64>,
    pub total_recommendations: i64,
    pub accepted_recommendations: i64,
    pub rejected_recommendations: i64,
    pub pending_recommendations: i64,
    pub acceptance_rate: Option<f64>,
    pub average_match_score: Option<f64>,
    pub top_patterns: Vec<PatternUsageMetric>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PatternRecommendationFeedback {
    pub accepted: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionRunExplanation {
    pub summary: String,
    pub decisions: Vec<DecisionRunDecisionExplanation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionRunDecisionExplanation {
    pub decision_id: String,
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
    #[serde(default)]
    pub requirement_ids: Vec<String>,
    #[serde(default)]
    pub matched_rule_requirements: Vec<MatchedRuleRequirementTrace>,
    pub result: serde_json::Value,
    pub explanation: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MatchedRuleRequirementTrace {
    pub rule_id: String,
    #[serde(default)]
    pub requirement_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionTraceMessage {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub decision: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorDiff {
    pub baseline_version_id: Option<i64>,
    pub candidate_version_id: Option<i64>,
    pub same_input: bool,
    pub status_changed: bool,
    pub execution_order_changed: bool,
    #[serde(default)]
    pub decision_diffs: Vec<DecisionDiff>,
    #[serde(default)]
    pub output_diffs: Vec<ValueDiff>,
    pub risk_level: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionDiff {
    pub decision_id: String,
    pub decision_name: String,
    pub change_type: String,
    pub baseline_order: Option<usize>,
    pub candidate_order: Option<usize>,
    pub matched_rules_changed: bool,
    #[serde(default)]
    pub baseline_matched_rule_ids: Vec<String>,
    #[serde(default)]
    pub candidate_matched_rule_ids: Vec<String>,
    pub baseline_result: Option<serde_json::Value>,
    pub candidate_result: Option<serde_json::Value>,
    pub risk_level: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ValueDiff {
    pub path: String,
    pub change_type: String,
    pub baseline_value: Option<serde_json::Value>,
    pub candidate_value: Option<serde_json::Value>,
    pub deviation_percent: Option<f64>,
    pub risk_level: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorComparisonEntry {
    pub baseline_run_id: i64,
    pub candidate_run_id: i64,
    pub diff: BehaviorDiff,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorComparisonReport {
    pub baseline_version_id: i64,
    pub candidate_version_id: i64,
    pub matched_cases: usize,
    #[serde(default)]
    pub baseline_sampled_runs: usize,
    #[serde(default)]
    pub baseline_total_runs: usize,
    #[serde(default)]
    pub candidate_sampled_runs: usize,
    #[serde(default)]
    pub candidate_total_runs: usize,
    pub unmatched_baseline_runs: usize,
    pub unmatched_candidate_runs: usize,
    #[serde(default)]
    pub comparisons: Vec<BehaviorComparisonEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RegressionTestCaseResult {
    pub source_run_id: i64,
    pub input: serde_json::Value,
    pub expected_output: serde_json::Value,
    pub actual_output: serde_json::Value,
    #[serde(default, skip_serializing)]
    pub execution_trace: serde_json::Value,
    pub status: String,
    pub risk_level: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RegressionTestReport {
    pub baseline_version_id: i64,
    pub candidate_version_id: i64,
    pub generated_count: usize,
    pub duplicate_count: usize,
    #[serde(default)]
    pub available_source_count: usize,
    #[serde(default)]
    pub source_coverage_percent: f64,
    pub passed: usize,
    pub failed: usize,
    pub errors: usize,
    pub risk_level: String,
    pub elapsed_ms: u64,
    #[serde(default)]
    pub cases: Vec<RegressionTestCaseResult>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OptimizationSimulationReport {
    pub simulation_id: i64,
    pub proposal_id: i64,
    pub baseline_version_id: i64,
    pub candidate_version_id: i64,
    pub sample_source: String,
    pub test_set_size: usize,
    #[serde(default)]
    pub minimum_sample_count: usize,
    pub baseline_accuracy: f64,
    pub simulated_accuracy: f64,
    pub accuracy_improvement: f64,
    pub bias_rate_baseline: f64,
    pub bias_rate_simulated: f64,
    #[serde(default)]
    pub passed: bool,
    #[serde(default)]
    pub proposal_withdrawn: bool,
    #[serde(default)]
    pub shadow_evaluation: Option<String>,
    #[serde(default)]
    pub autodraft_created_version_id: Option<i64>,
    #[serde(default)]
    pub autodraft_error: Option<String>,
    pub estimated_business_impact: String,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
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
#[serde(rename_all = "camelCase")]
pub struct PublicDecisionRunOutcome {
    #[serde(default)]
    pub business_reference: String,
    pub actual_outcome: serde_json::Value,
    pub assessment: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub occurred_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicDecisionRunOutcomeBatchItem {
    pub run_id: i64,
    pub idempotency_key: String,
    pub outcome: PublicDecisionRunOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicDecisionRunOutcomeBatch {
    pub service_id: String,
    pub items: Vec<PublicDecisionRunOutcomeBatchItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionRunOutcomeReceipt {
    pub run_id: i64,
    pub business_reference: String,
    pub assessment: String,
    pub reported_at: chrono::DateTime<chrono::Utc>,
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicDecisionRunOutcomeBatchResult {
    pub results: Vec<DecisionRunOutcomeReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicDecisionRunOutcomeReconciliationRequest {
    pub service_id: String,
    pub run_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct PublicDecisionRunOutcomeReconciliationItem {
    pub run_id: i64,
    pub state: String,
    pub business_reference: Option<String>,
    pub assessment: Option<String>,
    pub reported_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicDecisionRunOutcomeReconciliationResult {
    pub results: Vec<PublicDecisionRunOutcomeReconciliationItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct DeviationAnalysis {
    pub id: i64,
    pub project_id: i64,
    pub decision_run_id: i64,
    pub status: String,
    pub attributed_rule: Option<String>,
    pub confidence: f64,
    pub evidence: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub review_label: Option<String>,
    pub reviewed_by: Option<String>,
    pub reviewed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDeviationAnalysisReview {
    pub review_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviationAnalysisReviewResult {
    pub analysis_id: i64,
    pub review_label: String,
    pub reviewed_by: String,
    pub reviewed_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct DeviationAnalysisReviewRecord {
    pub id: i64,
    pub project_id: i64,
    pub decision_run_id: i64,
    pub status: String,
    pub attributed_rule: Option<String>,
    pub confidence: f64,
    pub review_label: String,
    pub reviewed_by: String,
    pub reviewed_at: chrono::DateTime<chrono::Utc>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct DecisionRunExplanationView {
    pub id: i64,
    pub decision_run_id: i64,
    pub generation_revision: i64,
    pub status: String,
    pub explanation_json: Option<serde_json::Value>,
    pub model: Option<String>,
    pub prompt_version: String,
    pub prompt_digest: String,
    pub trace_digest: String,
    pub error_message: Option<String>,
    pub created_by: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionRunExplanationReviewRequest {
    pub review_label: String,
    pub explanation_revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct DecisionRunExplanationReviewView {
    pub id: i64,
    pub explanation_revision: i64,
    pub review_label: String,
    pub reviewer_username: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct DecisionExplanationReviewExportRecord {
    pub id: i64,
    pub explanation_id: i64,
    pub explanation_revision: i64,
    pub reviewer_username: String,
    pub review_label: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionAnalyticsSummary {
    pub total_runs: usize,
    pub succeeded_runs: usize,
    pub failed_runs: usize,
    pub outcome_reported_runs: usize,
    #[serde(default)]
    pub conclusive_outcome_runs: usize,
    pub confirmed_runs: usize,
    pub mismatch_runs: usize,
    pub inconclusive_runs: usize,
    pub failure_rate: f64,
    pub feedback_coverage_rate: f64,
    pub mismatch_rate: f64,
    pub average_duration_ms: f64,
    pub p95_duration_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct DecisionAnalyticsThresholds {
    pub failure_warning_rate: f64,
    pub failure_high_rate: f64,
    pub failure_min_runs: i32,
    pub mismatch_warning_rate: f64,
    pub mismatch_high_rate: f64,
    pub mismatch_min_samples: i32,
    #[serde(default)]
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl Default for DecisionAnalyticsThresholds {
    fn default() -> Self {
        Self {
            failure_warning_rate: 0.10,
            failure_high_rate: 0.20,
            failure_min_runs: 10,
            mismatch_warning_rate: 0.20,
            mismatch_high_rate: 0.30,
            mismatch_min_samples: 5,
            updated_at: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionRuleHitMetric {
    pub rule_id: String,
    pub hit_count: usize,
    pub last_hit_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub feedback_count: usize,
    #[serde(default)]
    pub mismatch_count: usize,
    #[serde(default)]
    pub mismatch_rate: f64,
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
    #[serde(default)]
    pub details_omitted: bool,
}

#[cfg(test)]
mod result_distribution_tests {
    use super::*;

    #[test]
    fn historical_result_distributions_keep_their_detail_status() {
        let distribution: DecisionResultDistributionMetric =
            serde_json::from_value(serde_json::json!({
                "decisionId": "decision-1",
                "decisionName": "Decision",
                "total": 3,
                "values": [{"value": "true", "count": 3, "rate": 1.0}]
            }))
            .unwrap();

        assert!(!distribution.details_omitted);
        assert_eq!(distribution.total, 3);
        assert_eq!(distribution.values[0].count, 3);
    }

    #[test]
    fn omitted_result_details_survive_snapshot_round_trip() {
        let distribution = DecisionResultDistributionMetric {
            decision_id: "decision-1".into(),
            decision_name: "Decision".into(),
            total: 10,
            values: vec![DecisionResultValueMetric {
                value: "其他".into(),
                count: 10,
                rate: 1.0,
            }],
            details_omitted: true,
        };

        let json = serde_json::to_value(&distribution).unwrap();
        assert_eq!(json["detailsOmitted"], true);
        assert_eq!(
            serde_json::from_value::<DecisionResultDistributionMetric>(json).unwrap(),
            distribution
        );
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionDailyTrendMetric {
    pub date: chrono::NaiveDate,
    pub total_runs: usize,
    pub succeeded_runs: usize,
    pub failed_runs: usize,
    pub outcome_reported_runs: usize,
    #[serde(default)]
    pub conclusive_outcome_runs: usize,
    pub mismatch_runs: usize,
    pub average_duration_ms: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionAnalyticsFinding {
    pub level: String,
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub element: String,
    #[serde(default)]
    pub numeric_feedback_split: Option<NumericFeedbackSplitHint>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NumericFeedbackSplitHint {
    #[serde(default)]
    pub business_case_deduplicated: bool,
    pub input_field: String,
    pub threshold: f64,
    pub lower_mismatch_count: usize,
    pub lower_sample_count: usize,
    pub upper_mismatch_count: usize,
    pub upper_sample_count: usize,
}

impl DecisionAnalyticsFinding {
    pub fn numeric_feedback_split_summary(&self) -> Option<String> {
        let hint = self.numeric_feedback_split.as_ref()?;
        let lower_rate = percentage(hint.lower_mismatch_count, hint.lower_sample_count);
        let upper_rate = percentage(hint.upper_mismatch_count, hint.upper_sample_count);
        let sample_note = if hint.business_case_deduplicated {
            "样本按项目内业务关联号去重，每条规则保留各案件最近的明确反馈运行，去重不保证样本独立；"
        } else {
            ""
        };
        Some(format!(
            "数值输入关联线索：字段「{}」以 {:.4} 分组后，低值侧偏差率 {:.1}%（n={}），高值侧偏差率 {:.1}%（n={}）；{sample_note}仅为相关性线索，不能证明因果或直接作为规则阈值。",
            hint.input_field,
            hint.threshold,
            lower_rate,
            hint.lower_sample_count,
            upper_rate,
            hint.upper_sample_count,
        ))
    }
}

#[cfg(test)]
mod numeric_feedback_hint_tests {
    use super::*;

    #[test]
    fn older_numeric_hints_do_not_claim_business_case_deduplication() {
        let finding: DecisionAnalyticsFinding = serde_json::from_value(serde_json::json!({
            "level": "high",
            "code": "RULE_FEEDBACK_MISMATCH",
            "message": "historical finding",
            "numericFeedbackSplit": {
                "inputField": "age",
                "threshold": 24.5,
                "lowerMismatchCount": 5,
                "lowerSampleCount": 5,
                "upperMismatchCount": 0,
                "upperSampleCount": 5
            }
        }))
        .unwrap();

        assert!(
            !finding
                .numeric_feedback_split
                .as_ref()
                .unwrap()
                .business_case_deduplicated
        );
        assert!(
            !finding
                .numeric_feedback_split_summary()
                .unwrap()
                .contains("业务关联号去重")
        );
    }
}

fn percentage(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64 * 100.0
    }
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
    pub numeric_input_omitted_runs: usize,
    #[serde(default)]
    pub trace_details_omitted_runs: usize,
    #[serde(default)]
    pub unknown_result_steps_omitted: usize,
    #[serde(default)]
    pub rule_coverage: Vec<DecisionRuleCoverageMetric>,
    #[serde(default)]
    pub result_distributions: Vec<DecisionResultDistributionMetric>,
    #[serde(default)]
    pub daily_trends: Vec<DecisionDailyTrendMetric>,
    #[serde(default)]
    pub findings: Vec<DecisionAnalyticsFinding>,
    #[serde(default)]
    pub drift_signals: Vec<DecisionAnalyticsDriftSignal>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionAnalyticsDriftSignal {
    pub code: String,
    pub element: String,
    pub metric_value: f64,
    pub p_value: f64,
    pub baseline_sample_count: usize,
    pub recent_sample_count: usize,
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
    #[serde(default)]
    pub latest_simulation_passed: bool,
    #[serde(default)]
    pub effectiveness_result: Option<serde_json::Value>,
    #[serde(default)]
    pub effectiveness_tracked_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct DecisionDriftAutomationRule {
    pub project_id: i64,
    pub enabled: bool,
    pub finding_codes: Vec<String>,
    pub cooldown_hours: i32,
    pub webhook_url: Option<String>,
    pub next_eligible_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct DecisionDriftEvent {
    pub id: i64,
    pub project_id: i64,
    pub source_version_id: i64,
    pub dedupe_key: String,
    pub finding_codes: Vec<String>,
    pub evidence: serde_json::Value,
    #[serde(default)]
    pub action_result: Option<serde_json::Value>,
    #[serde(default)]
    pub webhook_delivery: Option<serde_json::Value>,
    #[serde(default)]
    pub review_label: Option<String>,
    #[serde(default)]
    pub reviewed_by: Option<String>,
    #[serde(default)]
    pub reviewed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub status: String,
    pub observed_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionDriftEventActionResult {
    pub event_id: i64,
    pub status: String,
    pub proposal_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDecisionDriftEventReview {
    pub review_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionDriftEventReviewResult {
    pub event_id: i64,
    pub review_label: String,
    pub reviewed_by: String,
    pub reviewed_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct DecisionDriftEventReviewRecord {
    pub id: i64,
    pub project_id: i64,
    pub source_version_id: i64,
    pub review_label: String,
    pub reviewed_by: String,
    pub reviewed_at: chrono::DateTime<chrono::Utc>,
    pub evidence: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct DecisionDriftScanWindow {
    pub id: i64,
    pub project_id: i64,
    pub source_version_id: i64,
    pub scan_day: chrono::NaiveDate,
    pub evidence: serde_json::Value,
    pub review_label: Option<String>,
    pub reviewed_by: Option<String>,
    pub reviewed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDecisionDriftScanWindowReview {
    pub review_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionDriftScanWindowReviewResult {
    pub window_id: i64,
    pub review_label: String,
    pub reviewed_by: String,
    pub reviewed_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct OptimizationAutodraftShadowEvaluation {
    pub id: i64,
    pub simulation_id: i64,
    pub project_id: i64,
    pub proposal_id: i64,
    pub source_version_id: i64,
    pub candidate_version_id: i64,
    pub outcome: String,
    pub finding_code: String,
    pub sample_source: String,
    pub test_set_size: i32,
    pub baseline_accuracy: f64,
    pub simulated_accuracy: f64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionCopilotQueryRequest {
    pub question: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionCopilotQueryResponse {
    pub status: String,
    pub answer: String,
    pub metric: Option<String>,
    pub window_days: Option<u32>,
    pub window_start: Option<chrono::DateTime<chrono::Utc>>,
    pub generated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub value: Option<f64>,
    pub unit: Option<String>,
    pub sample_size: Option<u64>,
    pub numerator: Option<u64>,
    pub denominator: Option<u64>,
    pub definition: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionDriftImpactReport {
    pub event_id: i64,
    pub source_version_id: i64,
    pub window_start: chrono::DateTime<chrono::Utc>,
    pub generated_at: chrono::DateTime<chrono::Utc>,
    pub total_runs: i64,
    pub succeeded_runs: i64,
    pub failed_runs: i64,
    pub feedback_runs: i64,
    pub mismatch_runs: i64,
    pub elements: Vec<DecisionDriftElementImpact>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
#[serde(rename_all = "camelCase")]
pub struct DecisionDriftElementImpact {
    pub element: String,
    pub observed_runs: i64,
    pub succeeded_runs: i64,
    pub feedback_runs: i64,
    pub mismatch_runs: i64,
    pub matched_rule_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDecisionDriftAutomation {
    pub enabled: bool,
    pub finding_codes: Vec<String>,
    pub cooldown_hours: i32,
    #[serde(default)]
    pub webhook_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishDecisionVersion {
    #[serde(default)]
    pub publish_confirmed: bool,
}
