use super::error::DmnError;
use crate::feel::FeelValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmnMessageSeverity {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DmnMessageSource {
    pub decision_id: Option<String>,
    pub decision_name: Option<String>,
    pub element_id: Option<String>,
    pub rule_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DmnMessage {
    pub severity: DmnMessageSeverity,
    pub code: &'static str,
    pub message: String,
    pub source: DmnMessageSource,
    pub error: Option<DmnError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionStatus {
    Succeeded,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmnResultStatus {
    Succeeded,
    SucceededWithWarnings,
    Failed,
}

#[derive(Debug, Clone)]
pub struct DmnResult {
    pub status: DmnResultStatus,
    pub decision_results: Vec<DecisionResult>,
    pub messages: Vec<DmnMessage>,
}

impl DmnResult {
    pub fn messages(&self) -> &[DmnMessage] {
        &self.messages
    }

    pub fn messages_by_severity(
        &self,
        severity: DmnMessageSeverity,
    ) -> impl Iterator<Item = &DmnMessage> {
        self.messages
            .iter()
            .filter(move |message| message.severity == severity)
    }

    pub fn errors(&self) -> impl Iterator<Item = &DmnMessage> {
        self.messages_by_severity(DmnMessageSeverity::Error)
    }

    pub fn warnings(&self) -> impl Iterator<Item = &DmnMessage> {
        self.messages_by_severity(DmnMessageSeverity::Warn)
    }

    pub fn has_errors(&self) -> bool {
        self.messages
            .iter()
            .any(|message| message.severity == DmnMessageSeverity::Error)
    }

    pub fn has_warnings(&self) -> bool {
        self.warnings().next().is_some()
    }

    pub fn first_error(&self) -> Option<&DmnMessage> {
        self.errors().next()
    }

    pub fn error_count(&self) -> usize {
        self.errors().count()
    }

    pub fn warning_count(&self) -> usize {
        self.warnings().count()
    }

    pub fn decision_by_id(&self, id: &str) -> Option<&DecisionResult> {
        self.decision_results
            .iter()
            .find(|decision| decision.decision_id == id)
    }

    pub fn decision_by_name(&self, name: &str) -> Option<&DecisionResult> {
        self.decision_results
            .iter()
            .find(|decision| decision.decision_name == name)
    }
}

#[derive(Debug, Clone)]
pub struct DecisionResult {
    pub decision_id: String,
    pub decision_name: String,
    pub value: FeelValue,
    pub matched_rule_ids: Vec<String>,
    pub status: DecisionStatus,
    pub evaluated_dependencies: Vec<String>,
}
