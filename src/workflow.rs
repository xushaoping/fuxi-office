use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionField {
    pub name: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub description: String,
    pub type_ref: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionTerm {
    pub term: String,
    pub meaning: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionBrief {
    pub name: String,
    pub goal: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub inputs: Vec<DecisionField>,
    #[serde(default)]
    pub outputs: Vec<DecisionField>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub terms: Vec<DecisionTerm>,
    #[serde(default)]
    pub assumptions: Vec<String>,
    #[serde(default = "default_risk_level")]
    pub risk_level: String,
}

fn default_risk_level() -> String {
    "medium".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Clarification {
    pub id: String,
    pub question: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClarificationAnswer {
    pub clarification_id: String,
    pub answer: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlannedDecision {
    pub name: String,
    pub purpose: String,
    pub output_type: String,
    pub expression_type: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlannedKnowledgeModel {
    pub name: String,
    pub purpose: String,
    #[serde(default)]
    pub parameters: Vec<String>,
    pub output_type: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlannedDecisionService {
    pub name: String,
    #[serde(default)]
    pub input_data: Vec<String>,
    #[serde(default)]
    pub encapsulated_decisions: Vec<String>,
    #[serde(default)]
    pub output_decisions: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionPlan {
    pub name: String,
    pub summary: String,
    #[serde(default)]
    pub decisions: Vec<PlannedDecision>,
    #[serde(default)]
    pub business_knowledge_models: Vec<PlannedKnowledgeModel>,
    #[serde(default)]
    pub decision_services: Vec<PlannedDecisionService>,
    #[serde(default)]
    pub defaults: Vec<String>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
}

/// A small, auditable plan describing a change to an existing DMN model.
/// This is intentionally separate from `DecisionPlan`: the latter describes
/// the target model for first-time generation, while this type describes only
/// the requested delta and its affected elements.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelChangePlan {
    pub summary: String,
    #[serde(default)]
    pub changes: Vec<ModelChange>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelChange {
    pub element_type: String,
    pub element: String,
    pub action: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequirementAnalysis {
    pub brief: DecisionBrief,
    #[serde(default)]
    pub clarifications: Vec<Clarification>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionWorkflowState {
    pub project_id: i64,
    pub status: String,
    pub requirement_text: String,
    pub brief: DecisionBrief,
    #[serde(default)]
    pub clarifications: Vec<Clarification>,
    #[serde(default)]
    pub answers: Vec<ClarificationAnswer>,
    pub plan: Option<DecisionPlan>,
    pub confirmed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeRequirementRequest {
    pub text: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDecisionPlanRequest {
    #[serde(default)]
    pub answers: Vec<ClarificationAnswer>,
}
