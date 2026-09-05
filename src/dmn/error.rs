use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DmnError {
    Xml(String),
    MissingAttribute {
        element: String,
        attribute: String,
    },
    DuplicateId(String),
    UnknownReference {
        decision: String,
        reference: String,
    },
    // 仅由预留的单决策执行 API（evaluate 等）与 artifact 测试路径构造
    #[cfg_attr(not(test), allow(dead_code))]
    UnknownDecision(String),
    UnknownDecisionService(String),
    MissingInput(String),
    CircularDependency(String),
    Feel {
        decision: String,
        detail: String,
    },
    InvalidDecisionTable {
        decision: String,
        detail: String,
    },
    InvalidDecisionService {
        service: String,
        detail: String,
    },
    InvalidTypeDefinition {
        type_name: String,
        detail: String,
    },
    UnknownTypeReference {
        owner: String,
        type_ref: String,
    },
    TypeConstraintViolation {
        location: String,
        type_ref: String,
        detail: String,
    },
    HitPolicyViolation {
        decision: String,
        detail: String,
    },
    UnsupportedFeature {
        decision: String,
        feature: String,
    },
    // 仅由已编译工件（artifact，测试构建）路径构造
    #[cfg_attr(not(test), allow(dead_code))]
    Artifact(String),
}

impl fmt::Display for DmnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Xml(detail) => write!(f, "DMN XML error: {detail}"),
            Self::MissingAttribute { element, attribute } => {
                write!(f, "<{element}> is missing required attribute '{attribute}'")
            }
            Self::DuplicateId(id) => write!(f, "duplicate DMN id '{id}'"),
            Self::UnknownReference {
                decision,
                reference,
            } => write!(
                f,
                "decision '{decision}' references unknown DRG element '{reference}'"
            ),
            Self::UnknownDecision(decision) => write!(f, "unknown decision '{decision}'"),
            Self::UnknownDecisionService(service) => {
                write!(f, "unknown decision service '{service}'")
            }
            Self::MissingInput(input) => write!(f, "missing required input '{input}'"),
            Self::CircularDependency(decision) => {
                write!(f, "circular dependency detected at decision '{decision}'")
            }
            Self::Feel { decision, detail } => write!(
                f,
                "FEEL evaluation failed for decision '{decision}': {detail}"
            ),
            Self::InvalidDecisionTable { decision, detail } => {
                write!(f, "invalid decision table for '{decision}': {detail}")
            }
            Self::InvalidDecisionService { service, detail } => {
                write!(f, "invalid decision service '{service}': {detail}")
            }
            Self::InvalidTypeDefinition { type_name, detail } => {
                write!(f, "invalid type definition '{type_name}': {detail}")
            }
            Self::UnknownTypeReference { owner, type_ref } => {
                write!(f, "'{owner}' references unknown type '{type_ref}'")
            }
            Self::TypeConstraintViolation {
                location,
                type_ref,
                detail,
            } => write!(
                f,
                "value at '{location}' does not conform to type '{type_ref}': {detail}"
            ),
            Self::HitPolicyViolation { decision, detail } => {
                write!(f, "hit policy violation for '{decision}': {detail}")
            }
            Self::UnsupportedFeature { decision, feature } => write!(
                f,
                "decision '{decision}' uses unsupported feature '{feature}'"
            ),
            Self::Artifact(detail) => write!(f, "invalid compiled DMN artifact: {detail}"),
        }
    }
}

impl std::error::Error for DmnError {}

pub type Result<T> = std::result::Result<T, DmnError>;
