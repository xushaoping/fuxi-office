use serde::{Deserialize, Serialize};

pub const MAX_MODEL_CHANGE_REPAIRS: i32 = 2;

#[cfg(any(test, target_family = "wasm"))]
pub fn is_repairable_model_change_error(message: &str, repair_attempt: i32) -> bool {
    (0..MAX_MODEL_CHANGE_REPAIRS).contains(&repair_attempt)
        && (message.starts_with("[DMN_COMPILE]") || message.starts_with("[PATCH_TARGET]"))
}

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
    #[serde(default)]
    pub base_revision: Option<i64>,
    pub status: String,
    pub requirement_text: String,
    #[serde(default)]
    pub original_requirement_text: Option<String>,
    pub brief: DecisionBrief,
    #[serde(default)]
    pub clarifications: Vec<Clarification>,
    #[serde(default)]
    pub answers: Vec<ClarificationAnswer>,
    pub plan: Option<DecisionPlan>,
    pub confirmed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl DecisionWorkflowState {
    pub fn matches_requirement(&self, requirement: &str, base_revision: Option<i64>) -> bool {
        self.requirement_text.trim() == requirement.trim() && self.base_revision == base_revision
    }

    pub fn generation_prompt(&self, base_revision: Option<i64>) -> Result<String, String> {
        if self.base_revision != base_revision {
            return Err("模型基线已变化，请重新分析需求并确认方案".into());
        }
        let plan = self
            .plan
            .as_ref()
            .filter(|plan| self.status == "confirmed" && !plan.decisions.is_empty())
            .ok_or_else(|| "请先完成需求分析、规划并确认决策方案".to_string())?;
        let task = if base_revision.is_some() {
            "将已确认方案落实为当前模型的最小变更意图。保留未涉及的元素和规则，不要整体重建模型。"
        } else {
            "将已确认方案转换为完整、可执行的 DMN DSL。"
        };
        let original_requirement = if base_revision.is_some() {
            self.original_requirement_text
                .as_deref()
                .unwrap_or("未留存原始需求，以当前 DSL 和已确认方案为准")
        } else {
            &self.requirement_text
        };
        Ok(format!(
            "## 生成任务\n{task}\n已确认方案是结构与规则依据，不得跳过、重新规划或自行扩展业务规则。原始需求仅作为业务背景；当前模型和已确认的变更方案优先，不得恢复已经调整过的旧规则。\n\n## 原始需求\n{original_requirement}\n\n## 本次业务需求\n{}\n\n## 已确认决策方案\n{}\n\n## 需求摘要\n{}\n\n## 已确认的业务回答\n{}",
            self.requirement_text,
            serde_json::to_string_pretty(plan).map_err(|error| error.to_string())?,
            serde_json::to_string_pretty(&self.brief).map_err(|error| error.to_string())?,
            serde_json::to_string_pretty(&self.answers).map_err(|error| error.to_string())?,
        ))
    }
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
    pub expected_updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmDecisionPlanRequest {
    pub expected_updated_at: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_change_repair_only_retries_fixable_preview_errors_within_limit() {
        for message in ["[PATCH_TARGET] 数组下标无效", "[DMN_COMPILE] 编译失败"] {
            assert!(is_repairable_model_change_error(message, 0));
            assert!(is_repairable_model_change_error(message, 1));
            assert!(!is_repairable_model_change_error(
                message,
                MAX_MODEL_CHANGE_REPAIRS
            ));
            assert!(!is_repairable_model_change_error(
                message,
                MAX_MODEL_CHANGE_REPAIRS + 1
            ));
            assert!(!is_repairable_model_change_error(message, -1));
        }
        for message in [
            "[BASE_CONFLICT] 草稿已更新",
            "[PATCH_CONFLICT] 基线不匹配",
            "[DSL_INVALID] DSL 校验未通过",
            "401 Unauthorized",
            "403 Forbidden",
            "网络连接失败",
            "服务异常 [PATCH_TARGET]",
            "[PATCH_TARGET_OTHER] 未知错误",
            "",
        ] {
            assert!(!is_repairable_model_change_error(message, 0), "{message}");
        }
    }

    fn workflow(base_revision: Option<i64>) -> DecisionWorkflowState {
        let timestamp = chrono::Utc::now();
        DecisionWorkflowState {
            project_id: 7,
            base_revision,
            status: "confirmed".into(),
            requirement_text: "订单满 500 元享受 95 折".into(),
            original_requirement_text: Some("订单满 300 元享受 95 折".into()),
            brief: DecisionBrief {
                goal: "判断订单优惠".into(),
                ..Default::default()
            },
            clarifications: Vec::new(),
            answers: vec![ClarificationAnswer {
                clarification_id: "stacking".into(),
                answer: "不叠加".into(),
            }],
            plan: Some(DecisionPlan {
                name: "OrderDiscount".into(),
                summary: "调整优惠门槛并保留客户等级规则".into(),
                decisions: vec![PlannedDecision {
                    name: "Discount".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            confirmed_at: Some(timestamp),
            updated_at: timestamp,
        }
    }

    #[test]
    fn requirement_match_requires_same_text_and_model_baseline() {
        let state = workflow(Some(42));
        assert!(state.matches_requirement(" 订单满 500 元享受 95 折\n", Some(42)));
        assert!(!state.matches_requirement("订单满 600 元享受 95 折", Some(42)));
        assert!(!state.matches_requirement(&state.requirement_text, Some(43)));
        assert!(!state.matches_requirement(&state.requirement_text, None));
    }

    #[test]
    fn new_model_prompt_uses_confirmed_plan_and_business_answers() {
        let prompt = workflow(None).generation_prompt(None).unwrap();
        assert!(prompt.contains("完整、可执行的 DMN DSL"));
        assert!(prompt.contains("OrderDiscount"));
        assert!(prompt.contains("不叠加"));
        assert!(prompt.contains("订单满 500 元享受 95 折"));
    }

    #[test]
    fn modification_prompt_preserves_unaffected_model_elements() {
        let prompt = workflow(Some(42)).generation_prompt(Some(42)).unwrap();
        assert!(prompt.contains("最小变更意图"));
        assert!(prompt.contains("保留未涉及的元素和规则"));
        assert!(prompt.contains("Discount"));
        assert!(prompt.contains("订单满 300 元享受 95 折"));
        assert!(prompt.contains("不得恢复已经调整过的旧规则"));
    }

    #[test]
    fn generation_rejects_every_unconfirmed_stage() {
        for status in ["analyzed", "clarification", "planned"] {
            let mut state = workflow(Some(42));
            state.status = status.into();
            assert!(state.generation_prompt(Some(42)).is_err(), "{status}");
        }
        let mut state = workflow(None);
        state.plan = None;
        assert!(state.generation_prompt(None).is_err());
        state.plan = Some(DecisionPlan::default());
        assert!(state.generation_prompt(None).is_err());
    }

    #[test]
    fn saved_or_switched_baseline_requires_new_analysis() {
        assert!(workflow(None).generation_prompt(Some(42)).is_err());
        assert!(workflow(Some(42)).generation_prompt(Some(43)).is_err());
        assert!(workflow(Some(42)).generation_prompt(None).is_err());
    }

    #[test]
    fn legacy_workflow_without_baseline_can_only_generate_new_model() {
        let mut payload = serde_json::to_value(workflow(None)).unwrap();
        payload.as_object_mut().unwrap().remove("baseRevision");
        payload
            .as_object_mut()
            .unwrap()
            .remove("originalRequirementText");
        let state: DecisionWorkflowState = serde_json::from_value(payload).unwrap();
        assert_eq!(state.base_revision, None);
        assert_eq!(state.original_requirement_text, None);
        assert!(state.generation_prompt(None).is_ok());
        assert!(state.generation_prompt(Some(42)).is_err());
    }

    #[test]
    fn plan_mutation_requests_require_loaded_workflow_timestamp() {
        assert!(
            serde_json::from_value::<CreateDecisionPlanRequest>(serde_json::json!({"answers": []}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<ConfirmDecisionPlanRequest>(serde_json::json!({})).is_err()
        );
        let timestamp = chrono::Utc::now();
        let plan: CreateDecisionPlanRequest = serde_json::from_value(serde_json::json!({
            "answers": [], "expectedUpdatedAt": timestamp,
        }))
        .unwrap();
        assert_eq!(plan.expected_updated_at, timestamp);
    }
}
