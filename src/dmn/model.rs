use super::error::{DmnError, Result};
use serde::{Deserialize, Serialize};

pub const DMN_NAMESPACE: &str = "https://www.omg.org/spec/DMN/20230324/MODEL/";
pub const DMN_20231201_NAMESPACE: &str = "https://www.omg.org/spec/DMN/20231201/MODEL/";
pub const DMN_20180521_NAMESPACE: &str = "http://www.omg.org/spec/DMN/20180521/MODEL/";

pub(crate) fn is_supported_dmn_namespace(namespace: &str) -> bool {
    matches!(
        namespace,
        DMN_NAMESPACE | DMN_20231201_NAMESPACE | DMN_20180521_NAMESPACE
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmnModel {
    pub name: Option<String>,
    pub namespace: Option<String>,
    #[serde(default)]
    pub expression_language: Option<String>,
    #[serde(default)]
    pub type_language: Option<String>,
    pub item_definitions: Vec<ItemDefinition>,
    pub input_data: Vec<InputData>,
    pub decisions: Vec<Decision>,
    pub business_knowledge_models: Vec<BusinessKnowledgeModel>,
    pub imports: Vec<Import>,
    pub decision_services: Vec<DecisionService>,
}

pub(crate) fn is_feel_language(language: &str) -> bool {
    language.eq_ignore_ascii_case("FEEL") || language.trim_end_matches('/').ends_with("/FEEL")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemDefinition {
    pub id: Option<String>,
    pub name: String,
    pub type_ref: Option<String>,
    pub is_collection: bool,
    pub allowed_values: Option<String>,
    pub item_components: Vec<ItemDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputData {
    pub id: String,
    pub name: String,
    pub variable: Option<String>,
    pub type_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub id: String,
    pub name: String,
    pub variable: Option<String>,
    pub type_ref: Option<String>,
    pub requirements: Vec<InformationRequirement>,
    #[serde(default)]
    pub knowledge_requirements: Vec<KnowledgeRequirement>,
    pub logic: DecisionLogic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InformationRequirement {
    pub id: Option<String>,
    pub required_input: Option<String>,
    pub required_decision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeRequirement {
    pub id: Option<String>,
    pub required_knowledge: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DecisionLogic {
    DecisionTable(DecisionTable),
    LiteralExpression(String),
    Context(Vec<ContextEntry>),
    List(Vec<DecisionLogic>),
    Relation(Relation),
    Conditional(Conditional),
    Filter(FilterExpression),
    For(IteratorExpression),
    Some(QuantifiedExpression),
    Every(QuantifiedExpression),
    Invocation(Invocation),
    FunctionDefinition(FunctionDefinition),
    Unsupported(String),
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conditional {
    pub condition: Box<DecisionLogic>,
    pub then_expression: Box<DecisionLogic>,
    pub else_expression: Box<DecisionLogic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterExpression {
    pub input: Box<DecisionLogic>,
    pub matcher: Box<DecisionLogic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IteratorExpression {
    pub variable: String,
    pub input: Box<DecisionLogic>,
    pub return_expression: Box<DecisionLogic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantifiedExpression {
    pub variable: String,
    pub input: Box<DecisionLogic>,
    pub satisfies: Box<DecisionLogic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    pub id: Option<String>,
    pub columns: Vec<RelationColumn>,
    pub rows: Vec<RelationRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationColumn {
    pub id: Option<String>,
    pub name: String,
    pub type_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationRow {
    pub id: Option<String>,
    pub entries: Vec<DecisionLogic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEntry {
    pub name: String,
    #[serde(default)]
    pub type_ref: Option<String>,
    pub expression: DecisionLogic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invocation {
    pub expression: Box<DecisionLogic>,
    pub bindings: Vec<Binding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    pub parameter: String,
    pub expression: DecisionLogic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub parameters: Vec<String>,
    #[serde(default)]
    pub parameter_type_refs: Vec<Option<String>>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub result_type_ref: Option<String>,
    pub body: Box<DecisionLogic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessKnowledgeModel {
    pub id: String,
    pub name: String,
    pub variable: Option<String>,
    #[serde(default)]
    pub type_ref: Option<String>,
    #[serde(default)]
    pub knowledge_requirements: Vec<KnowledgeRequirement>,
    pub logic: DecisionLogic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    pub id: Option<String>,
    pub name: Option<String>,
    pub namespace: Option<String>,
    pub location_uri: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionService {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub output_decisions: Vec<String>,
    #[serde(default)]
    pub encapsulated_decisions: Vec<String>,
    #[serde(default)]
    pub input_decisions: Vec<String>,
    #[serde(default)]
    pub input_data: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionTable {
    pub id: Option<String>,
    pub hit_policy: HitPolicy,
    pub aggregation: Option<Aggregation>,
    pub inputs: Vec<InputClause>,
    pub outputs: Vec<OutputClause>,
    pub rules: Vec<DecisionRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputClause {
    pub expression: String,
    pub variable: Option<String>,
    pub type_ref: Option<String>,
    /// Optional unary tests constraining the input expression result.
    #[serde(default)]
    pub input_values: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputClause {
    pub name: Option<String>,
    pub type_ref: Option<String>,
    pub default_output_entry: Option<String>,
    pub output_values: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRule {
    pub id: Option<String>,
    pub input_entries: Vec<String>,
    pub output_entries: Vec<String>,
    pub annotation_entries: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HitPolicy {
    Unique,
    First,
    Any,
    Priority,
    RuleOrder,
    OutputOrder,
    Collect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Aggregation {
    Sum,
    Min,
    Max,
    Count,
}

impl HitPolicy {
    pub(crate) fn parse(value: Option<&str>) -> Result<Self> {
        match value.unwrap_or("UNIQUE") {
            "UNIQUE" => Ok(Self::Unique),
            "FIRST" => Ok(Self::First),
            "ANY" => Ok(Self::Any),
            "PRIORITY" => Ok(Self::Priority),
            "RULE ORDER" => Ok(Self::RuleOrder),
            "OUTPUT ORDER" => Ok(Self::OutputOrder),
            "COLLECT" => Ok(Self::Collect),
            other => Err(DmnError::Xml(format!("unsupported hitPolicy '{other}'"))),
        }
    }
}

impl Aggregation {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "SUM" => Ok(Self::Sum),
            "MIN" => Ok(Self::Min),
            "MAX" => Ok(Self::Max),
            "COUNT" => Ok(Self::Count),
            other => Err(DmnError::Xml(format!("unsupported aggregation '{other}'"))),
        }
    }
}
