use super::error::DmnError;
use super::model::{Decision, DecisionRule, DecisionTable};
use super::result::*;
use crate::feel::FeelValue;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub(crate) struct TableEvaluation {
    pub(crate) value: FeelValue,
    pub(crate) matched_rule_ids: Vec<String>,
}

pub(crate) struct ExecutionState {
    pub(crate) context: HashMap<String, FeelValue>,
    pub(crate) decisions: HashMap<String, DecisionResult>,
    pub(crate) failure_source: Option<DmnMessageSource>,
    pub(crate) validated_inputs: HashMap<String, std::result::Result<(), DmnError>>,
}

pub(crate) struct DmnDiagnostics {
    messages: Vec<DmnMessage>,
}

impl DmnDiagnostics {
    pub(crate) fn new() -> Self {
        Self {
            messages: Vec::new(),
        }
    }

    pub(crate) fn push(&mut self, message: DmnMessage) {
        self.messages.push(message);
    }

    pub(crate) fn error(&mut self, error: DmnError, source: DmnMessageSource) {
        self.push(message_from_error(error, source));
    }

    pub(crate) fn dependency_failure(&mut self, source: DmnMessageSource) {
        self.push(DmnMessage {
            severity: DmnMessageSeverity::Error,
            code: "DMN-EVAL-DEPENDENCY",
            message: "a required input or decision failed".into(),
            source,
            error: None,
        });
    }

    pub(crate) fn into_messages(self) -> Vec<DmnMessage> {
        self.messages
    }
}

pub(crate) fn decision_source(decision: &Decision) -> DmnMessageSource {
    DmnMessageSource {
        decision_id: Some(decision.id.clone()),
        decision_name: Some(decision.name.clone()),
        element_id: Some(decision.id.clone()),
        rule_id: None,
    }
}

pub(crate) fn decision_table_source(
    decision: &Decision,
    table: &DecisionTable,
) -> DmnMessageSource {
    let mut source = decision_source(decision);
    source.element_id = table.id.clone().or(source.element_id);
    source
}

pub(crate) fn rule_source(
    decision: &Decision,
    table: &DecisionTable,
    rule: &DecisionRule,
) -> DmnMessageSource {
    let mut source = decision_table_source(decision, table);
    source.rule_id = rule.id.clone();
    if rule.id.is_some() {
        source.element_id = rule.id.clone();
    }
    source
}

pub(crate) fn message_from_error(error: DmnError, source: DmnMessageSource) -> DmnMessage {
    let (code, message) = match &error {
        DmnError::MissingInput(_) => ("DMN-EVAL-MISSING-INPUT", error.to_string()),
        DmnError::Feel { .. } => ("DMN-EVAL-FEEL", error.to_string()),
        DmnError::HitPolicyViolation { .. } => ("DMN-EVAL-HIT-POLICY", error.to_string()),
        DmnError::UnsupportedFeature { .. } => ("DMN-EVAL-UNSUPPORTED", error.to_string()),
        DmnError::InvalidDecisionTable { .. } => ("DMN-EVAL-TABLE", error.to_string()),
        DmnError::TypeConstraintViolation { .. } => ("DMN-EVAL-TYPE", error.to_string()),
        _ => ("DMN-EVAL-ERROR", error.to_string()),
    };
    DmnMessage {
        severity: DmnMessageSeverity::Error,
        code,
        message,
        source,
        error: Some(error),
    }
}

pub(crate) fn result_from(
    decision_results: Vec<DecisionResult>,
    diagnostics: DmnDiagnostics,
) -> DmnResult {
    let messages = diagnostics.into_messages();
    let status = if messages
        .iter()
        .any(|message| message.severity == DmnMessageSeverity::Error)
    {
        DmnResultStatus::Failed
    } else if messages
        .iter()
        .any(|message| message.severity == DmnMessageSeverity::Warn)
    {
        DmnResultStatus::SucceededWithWarnings
    } else {
        DmnResultStatus::Succeeded
    };
    DmnResult {
        status,
        decision_results,
        messages,
    }
}

pub(crate) fn failed_result(error: DmnError, source: DmnMessageSource) -> DmnResult {
    let mut diagnostics = DmnDiagnostics::new();
    diagnostics.error(error, source);
    result_from(Vec::new(), diagnostics)
}

#[cfg_attr(not(test), allow(dead_code))] // 供单决策执行（evaluate）预留 API 提取首个错误
pub(crate) fn result_error(result: &DmnResult) -> Option<DmnError> {
    result
        .messages
        .iter()
        .find_map(|message| message.error.clone())
}
