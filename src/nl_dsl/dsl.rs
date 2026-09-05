use petgraph::algo::toposort;
use petgraph::graph::DiGraph;
use serde::{Deserialize, Serialize};

/// DMN Definitions 根容器
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Definitions {
    pub name: String,
    #[serde(default = "default_namespace")]
    pub namespace: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub imports: Vec<Import>, // 新增：支持外部模型导入
    #[serde(default)]
    pub item_definitions: Vec<ItemDefinition>,
    #[serde(default)]
    pub inputs: Vec<InputData>,
    #[serde(default)]
    pub decisions: Vec<Decision>,
    #[serde(default)]
    pub bkms: Vec<BusinessKnowledgeModel>,
    #[serde(default)]
    pub decision_services: Vec<DecisionService>,
    #[serde(default)]
    pub knowledge_sources: Vec<KnowledgeSource>,
    #[serde(default)]
    pub text_annotations: Vec<TextAnnotation>,
    #[serde(default)]
    pub diagram: Option<Diagram>,
}

// 新增：导入外部模型定义
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Import {
    pub name: String,
    pub namespace: String,
    pub location: String,
    pub import_type: String,
}

fn default_namespace() -> String {
    "http://example.com/dmn".to_string()
}

fn is_standard_type_ref(value: &str) -> bool {
    matches!(
        value,
        "string" | "number" | "boolean" | "date" | "time" | "dateTime" | "duration" | "Any"
    )
}

fn validate_type_ref(
    owner: &str,
    type_ref: &str,
    item_definition_names: &std::collections::HashSet<&str>,
) -> anyhow::Result<()> {
    let type_ref = type_ref.trim();
    if type_ref.is_empty() {
        anyhow::bail!("{} 的 typeRef 不能为空", owner);
    }

    let is_known = if let Some(feel_type) = type_ref.strip_prefix("feel:") {
        is_standard_type_ref(feel_type)
    } else {
        is_standard_type_ref(type_ref)
            || item_definition_names.contains(type_ref)
            // Imported/external type references cannot be resolved within one model.
            || type_ref.contains(':')
    };
    if !is_known {
        anyhow::bail!("{} 引用了未定义的 typeRef: '{}'", owner, type_ref);
    }
    Ok(())
}

fn validate_item_type(
    item_type: &ItemType,
    owner: &str,
    item_definition_names: &std::collections::HashSet<&str>,
) -> anyhow::Result<()> {
    match item_type {
        ItemType::Simple { type_ref } => validate_type_ref(owner, type_ref, item_definition_names),
        ItemType::Enumeration { type_ref, values } => {
            validate_type_ref(owner, type_ref, item_definition_names)?;
            if values.is_empty() {
                anyhow::bail!("{} 的枚举值不能为空", owner);
            }
            let mut seen = std::collections::HashSet::new();
            for value in values {
                if value.trim().is_empty() || !seen.insert(value) {
                    anyhow::bail!("{} 包含空白或重复的枚举值: '{}'", owner, value);
                }
            }
            Ok(())
        }
        ItemType::Structure { fields } => {
            let mut field_names = std::collections::HashSet::new();
            for field in fields {
                if field.name.trim().is_empty() || !field_names.insert(&field.name) {
                    anyhow::bail!("{} 包含空白或重复字段名: '{}'", owner, field.name);
                }
                validate_type_ref(
                    &format!("{} 的字段 '{}'", owner, field.name),
                    &field.type_,
                    item_definition_names,
                )?;
            }
            Ok(())
        }
        ItemType::List { item_type } => validate_item_type(item_type, owner, item_definition_names),
    }
}

fn validate_expression_type_refs(
    expression: &BoxedExpression,
    owner: &str,
    item_definition_names: &std::collections::HashSet<&str>,
) -> anyhow::Result<()> {
    match expression {
        BoxedExpression::DecisionTable {
            inputs, outputs, ..
        } => {
            if outputs.is_empty() {
                anyhow::bail!("{} 的决策表至少需要一个 output", owner);
            }
            for input in inputs {
                if let Some(type_ref) = &input.type_ref {
                    validate_type_ref(
                        &format!("{} 的决策表输入", owner),
                        type_ref,
                        item_definition_names,
                    )?;
                }
            }
            for output in outputs {
                validate_type_ref(
                    &format!("{} 的决策表输出 '{}'", owner, output.name),
                    &output.type_ref,
                    item_definition_names,
                )?;
            }
        }
        BoxedExpression::Context { entries, .. } => {
            for entry in entries {
                validate_expression_type_refs(
                    &entry.value,
                    &format!("{} 的 context 条目 '{}'", owner, entry.key),
                    item_definition_names,
                )?;
            }
        }
        BoxedExpression::FunctionDefinition { parameters, body } => {
            for parameter in parameters {
                validate_type_ref(
                    &format!("{} 的函数参数 '{}'", owner, parameter.name),
                    &parameter.type_ref,
                    item_definition_names,
                )?;
            }
            validate_expression_type_refs(body, owner, item_definition_names)?;
        }
        BoxedExpression::LiteralExpression { type_ref, .. } => {
            if let Some(type_ref) = type_ref {
                validate_type_ref(owner, type_ref, item_definition_names)?;
            }
        }
        BoxedExpression::Conditional { then, else_, .. } => {
            validate_expression_type_refs(then, owner, item_definition_names)?;
            validate_expression_type_refs(else_, owner, item_definition_names)?;
        }
        BoxedExpression::Invocation { .. }
        | BoxedExpression::List { .. }
        | BoxedExpression::Relation { .. }
        | BoxedExpression::Filter { .. }
        | BoxedExpression::For { .. }
        | BoxedExpression::Quantified { .. } => {}
    }
    Ok(())
}

/// ItemDefinition - 自定义数据类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemDefinition {
    pub name: String,
    #[serde(default)]
    pub is_collection: bool,
    #[serde(rename = "type")]
    pub type_: ItemType,
}

/// ItemType - 自定义数据类型
///
/// 序列化输出统一为 internal tag 格式（{"kind": "...", ...}）；
/// 反序列化做宽容处理，兼容 LLM 常见的写法：
///   1. 标准 internal tag：{"kind": "structure", "fields": [...]}
///   2. external tag：{"structure": {"fields": [...]}} / {"structure": [...]}（值直接是字段数组）
///   3. simple 缩写：{"simple": "number"}（值直接是类型字符串）
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ItemType {
    Simple {
        type_ref: String,
    },
    Structure {
        fields: Vec<ItemField>,
    },
    List {
        item_type: Box<ItemType>,
    },
    Enumeration {
        type_ref: String,
        values: Vec<String>,
    },
}

/// 反序列化镜像（严格 internal tag 格式），嵌套 itemType 复用公开枚举递归宽容解析
/// 字段主键 camelCase（LLM 习惯），alias 兼容历史 YAML 的 snake_case 写法
#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
enum ItemTypeRaw {
    Simple {
        #[serde(alias = "type_ref")]
        type_ref: String,
    },
    Structure {
        fields: Vec<ItemField>,
    },
    List {
        #[serde(alias = "item_type")]
        item_type: Box<ItemType>,
    },
    Enumeration {
        #[serde(alias = "type_ref")]
        type_ref: String,
        values: Vec<String>,
    },
}

impl<'de> Deserialize<'de> for ItemType {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let v = serde_json::Value::deserialize(d)?;
        let normalized = normalize_item_type(&v)
            .ok_or_else(|| D::Error::custom(format!("无法识别的 ItemType 格式: {}", v)))?;
        serde_json::from_value::<ItemTypeRaw>(normalized)
            .map(ItemType::from)
            .map_err(|e| D::Error::custom(format!("ItemType 字段不合法: {}", e)))
    }
}

impl From<ItemTypeRaw> for ItemType {
    fn from(raw: ItemTypeRaw) -> Self {
        match raw {
            ItemTypeRaw::Simple { type_ref } => ItemType::Simple { type_ref },
            ItemTypeRaw::Structure { fields } => ItemType::Structure { fields },
            ItemTypeRaw::List { item_type } => ItemType::List { item_type },
            ItemTypeRaw::Enumeration { type_ref, values } => {
                ItemType::Enumeration { type_ref, values }
            }
        }
    }
}

/// 将 LLM 各种 ItemType 写法归一化为标准 internal tag 对象（{"kind": "...", ...}）
fn normalize_item_type(v: &serde_json::Value) -> Option<serde_json::Value> {
    let obj = v.as_object()?;

    // internal tag 格式：{"kind": "structure", "fields": [...]} 原样返回
    if obj.contains_key("kind") {
        return Some(v.clone());
    }

    // external tag 格式：单键对象 {"structure": {...}} / {"structure": [...]} / {"simple": "number"}
    if obj.len() == 1
        && let Some((k, inner)) = obj.iter().next()
    {
        if !is_item_type_variant(k) {
            return None;
        }
        let mut o = serde_json::Map::new();
        o.insert("kind".to_string(), serde_json::Value::String(k.clone()));
        match inner {
            serde_json::Value::Object(inner_obj) => {
                for (kk, vv) in inner_obj {
                    o.insert(kk.clone(), vv.clone());
                }
            }
            serde_json::Value::Array(items) if k == "structure" => {
                // {"structure": [{"name": ..., "typeRef": ...}]}：值直接是字段数组
                o.insert(
                    "fields".to_string(),
                    serde_json::Value::Array(items.clone()),
                );
            }
            serde_json::Value::String(s) if k == "simple" => {
                // {"simple": "number"}：值直接是类型字符串
                o.insert("typeRef".to_string(), serde_json::Value::String(s.clone()));
            }
            _ => return None,
        }
        return Some(serde_json::Value::Object(o));
    }

    None
}

fn is_item_type_variant(name: &str) -> bool {
    matches!(name, "simple" | "structure" | "list" | "enumeration")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemField {
    pub name: String,
    // LLM 常写 typeRef（与 DSL 其他处一致），反序列化时两者都接受
    #[serde(rename = "type", alias = "typeRef")]
    pub type_: String,
    #[serde(default)]
    pub is_collection: bool,
}

/// InputData - 输入数据
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputData {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub type_ref: Option<String>,
}

/// Decision - 决策
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub type_ref: Option<String>,
    #[serde(default)]
    pub required_inputs: Vec<String>,
    #[serde(default)]
    pub required_decisions: Vec<String>,
    #[serde(default)]
    pub required_knowledge: Vec<String>,
    pub logic: BoxedExpression,
}

/// BusinessKnowledgeModel - 业务知识模型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BusinessKnowledgeModel {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub type_ref: Option<String>,
    #[serde(default)]
    pub parameters: Vec<FormalParameter>,
    pub logic: BoxedExpression,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormalParameter {
    pub name: String,
    pub type_ref: String,
    #[serde(default)]
    pub is_collection: bool,
}

/// DecisionService - 决策服务
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionService {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub output_decisions: Vec<String>,
    #[serde(default)]
    pub encapsulated_decisions: Vec<String>,
    #[serde(default)]
    pub input_data: Vec<String>,
}

/// KnowledgeSource - 知识源
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSource {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub authority_targets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextAnnotation {
    pub text: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagram {
    #[serde(default)]
    pub nodes: Vec<DiagramNode>,
    #[serde(default)]
    pub edges: Vec<DiagramEdge>, // 新增：支持连线布局
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagramNode {
    #[serde(alias = "name")]
    pub element: String,
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub width: Option<f64>,
    #[serde(default)]
    pub height: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagramEdge {
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub waypoints: Vec<(f64, f64)>,
}

/// BoxedExpression - 涵盖 DMN 1.5 所有盒装表达式
///
/// 序列化输出统一为 internal tag 格式（{"type": "...", ...}）；
/// 反序列化做宽容处理，兼容 LLM 常见的三种写法：
///   1. 标准 internal tag：{"type": "literalExpression", "expression": "a+b", "typeRef": "number"}
///   2. internal tag + text 别名：{"type": "literalExpression", "text": "a+b", ...}
///   3. external tag：{"literalExpression": {"expression": "a+b", ...}}（单键对象）
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum BoxedExpression {
    DecisionTable {
        #[serde(default, alias = "hit_policy")]
        hit_policy: Option<String>,
        #[serde(default)]
        aggregation: Option<String>,
        #[serde(default, alias = "preferred_orientation")]
        preferred_orientation: Option<String>,
        #[serde(default)]
        inputs: Vec<DtInput>,
        outputs: Vec<DtOutput>,
        #[serde(default)]
        rules: Vec<DtRule>,
    },
    Context {
        entries: Vec<ContextEntry>,
        #[serde(default, alias = "return_entry")]
        return_entry: Option<String>,
    },
    FunctionDefinition {
        #[serde(default)]
        parameters: Vec<FormalParameter>,
        body: Box<BoxedExpression>,
    },
    Invocation {
        bkm: String,
        #[serde(default)]
        bindings: Vec<Binding>,
    },
    LiteralExpression {
        expression: String,
        #[serde(default, alias = "type_ref")]
        type_ref: Option<String>,
    },
    List {
        #[serde(default)]
        items: Vec<String>,
    },
    Relation {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Conditional {
        condition: String,
        then: Box<BoxedExpression>,
        #[serde(rename = "else")]
        else_: Box<BoxedExpression>,
    },
    Filter {
        source: String,
        filter: String,
    },
    For {
        variable: String,
        #[serde(rename = "in")]
        in_: String,
        #[serde(rename = "return")]
        return_: String,
    },
    Quantified {
        kind: String, // "some" | "every"
        variable: String,
        #[serde(rename = "in")]
        in_: String,
        satisfies: String,
    },
}

/// 反序列化镜像（严格 internal tag 格式），避免宽容解析无限递归
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum BoxedExpressionRaw {
    DecisionTable {
        #[serde(default, alias = "hit_policy")]
        hit_policy: Option<String>,
        #[serde(default)]
        aggregation: Option<String>,
        #[serde(default, alias = "preferred_orientation")]
        preferred_orientation: Option<String>,
        #[serde(default)]
        inputs: Vec<DtInput>,
        outputs: Vec<DtOutput>,
        #[serde(default)]
        rules: Vec<DtRule>,
    },
    Context {
        entries: Vec<ContextEntry>,
        #[serde(default, alias = "return_entry")]
        return_entry: Option<String>,
    },
    FunctionDefinition {
        #[serde(default)]
        parameters: Vec<FormalParameter>,
        body: Box<BoxedExpression>,
    },
    Invocation {
        bkm: String,
        #[serde(default)]
        bindings: Vec<Binding>,
    },
    LiteralExpression {
        expression: String,
        #[serde(default, alias = "type_ref")]
        type_ref: Option<String>,
    },
    List {
        #[serde(default)]
        items: Vec<String>,
    },
    Relation {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Conditional {
        condition: String,
        then: Box<BoxedExpression>,
        #[serde(rename = "else")]
        else_: Box<BoxedExpression>,
    },
    Filter {
        source: String,
        filter: String,
    },
    For {
        variable: String,
        #[serde(rename = "in")]
        in_: String,
        #[serde(rename = "return")]
        return_: String,
    },
    Quantified {
        kind: String, // "some" | "every"
        variable: String,
        #[serde(rename = "in")]
        in_: String,
        satisfies: String,
    },
}

impl<'de> Deserialize<'de> for BoxedExpression {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let v = serde_json::Value::deserialize(d)?;
        let normalized = normalize_boxed_expression(&v)
            .ok_or_else(|| D::Error::custom(format!("无法识别的 BoxedExpression 格式: {}", v)))?;
        serde_json::from_value::<BoxedExpressionRaw>(normalized)
            .map(BoxedExpression::from)
            .map_err(|e| D::Error::custom(format!("BoxedExpression 字段不合法: {}", e)))
    }
}

impl From<BoxedExpressionRaw> for BoxedExpression {
    fn from(raw: BoxedExpressionRaw) -> Self {
        match raw {
            BoxedExpressionRaw::DecisionTable {
                hit_policy,
                aggregation,
                preferred_orientation,
                inputs,
                outputs,
                rules,
            } => BoxedExpression::DecisionTable {
                hit_policy,
                aggregation,
                preferred_orientation,
                inputs,
                outputs,
                rules,
            },
            BoxedExpressionRaw::Context {
                entries,
                return_entry,
            } => BoxedExpression::Context {
                entries,
                return_entry,
            },
            BoxedExpressionRaw::FunctionDefinition { parameters, body } => {
                BoxedExpression::FunctionDefinition { parameters, body }
            }
            BoxedExpressionRaw::Invocation { bkm, bindings } => {
                BoxedExpression::Invocation { bkm, bindings }
            }
            BoxedExpressionRaw::LiteralExpression {
                expression,
                type_ref,
            } => BoxedExpression::LiteralExpression {
                expression,
                type_ref,
            },
            BoxedExpressionRaw::List { items } => BoxedExpression::List { items },
            BoxedExpressionRaw::Relation { columns, rows } => {
                BoxedExpression::Relation { columns, rows }
            }
            BoxedExpressionRaw::Conditional {
                condition,
                then,
                else_,
            } => BoxedExpression::Conditional {
                condition,
                then,
                else_,
            },
            BoxedExpressionRaw::Filter { source, filter } => {
                BoxedExpression::Filter { source, filter }
            }
            BoxedExpressionRaw::For {
                variable,
                in_,
                return_,
            } => BoxedExpression::For {
                variable,
                in_,
                return_,
            },
            BoxedExpressionRaw::Quantified {
                kind,
                variable,
                in_,
                satisfies,
            } => BoxedExpression::Quantified {
                kind,
                variable,
                in_,
                satisfies,
            },
        }
    }
}

/// 将 LLM 各种写法归一化为标准 internal tag 对象
fn normalize_boxed_expression(v: &serde_json::Value) -> Option<serde_json::Value> {
    let obj = v.as_object()?;

    // internal tag 格式：处理 LiteralExpression 的字段别名（text/value/body/literal → expression）
    if let Some(t) = obj.get("type").and_then(|x| x.as_str()) {
        if t == "literalExpression" && !obj.contains_key("expression") {
            for alias in ["text", "value", "body", "literal"] {
                if let Some(alias_val) = obj.get(alias) {
                    let mut o = obj.clone();
                    o.insert("expression".to_string(), alias_val.clone());
                    return Some(serde_json::Value::Object(o));
                }
            }
        }
        return Some(v.clone());
    }

    // external tag 格式：{"literalExpression": {...}} 单键对象
    if obj.len() == 1
        && let Some((k, inner)) = obj.iter().next()
        && is_boxed_variant(k)
    {
        let inner_obj = inner.as_object()?;
        let mut o = serde_json::Map::new();
        o.insert("type".to_string(), serde_json::Value::String(k.clone()));
        for (kk, vv) in inner_obj {
            o.insert(kk.clone(), vv.clone());
        }
        // 递归归一化：展开后已是 internal tag 形式，可继续处理 text → expression 别名
        return normalize_boxed_expression(&serde_json::Value::Object(o));
    }

    None
}

fn is_boxed_variant(name: &str) -> bool {
    matches!(
        name,
        "decisionTable"
            | "context"
            | "functionDefinition"
            | "invocation"
            | "literalExpression"
            | "list"
            | "relation"
            | "conditional"
            | "filter"
            | "for"
            | "quantified"
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DtInput {
    #[serde(default, alias = "label")]
    pub name: Option<String>,
    pub expression: String,
    #[serde(default)]
    pub type_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DtOutput {
    pub name: String,
    pub type_ref: String,
    #[serde(default)]
    pub default_output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DtRule {
    pub input_entries: Vec<String>,
    pub output_entries: Vec<String>,
    #[serde(default)]
    pub annotation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextEntry {
    #[serde(alias = "name")]
    pub key: String,
    pub value: BoxedExpression,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub parameter: String,
    pub expression: String,
}

/// 解析 DSL YAML，兼容顶层带 `definitions:` 包装层与不带包装两种格式
pub fn parse_definitions(yaml: &str) -> anyhow::Result<Definitions> {
    match serde_yaml::from_str::<Definitions>(yaml) {
        Ok(mut d) => {
            normalize_type_refs(&mut d);
            normalize_dependency_references(&mut d);
            Ok(d)
        }
        Err(e) => {
            // 直接解析失败时，尝试剥掉 `definitions:` 包装层再解析
            let value: serde_yaml::Value =
                serde_yaml::from_str(yaml).map_err(|_| anyhow::anyhow!("YAML 解析失败: {}", e))?;
            let inner = value
                .get("definitions")
                .ok_or_else(|| anyhow::anyhow!("YAML 解析失败: {}", e))?;
            let mut d: Definitions = serde_yaml::from_value(inner.clone())
                .map_err(|e2| anyhow::anyhow!("definitions 包装层解析失败: {}", e2))?;
            normalize_type_refs(&mut d);
            normalize_dependency_references(&mut d);
            Ok(d)
        }
    }
}

/// Normalize common LLM numeric aliases to FEEL's single numeric type.
///
/// FEEL uses `number`; models frequently emit `integer`, `int`, `long`, or
/// `double` for numeric fields. Keeping the normalization at the DSL boundary
/// makes JSON/YAML generation and ChangeIntent application behave identically
/// while preserving a user-defined itemDefinition with the same name.
pub fn normalize_type_refs(definitions: &mut Definitions) {
    let custom_types = definitions
        .item_definitions
        .iter()
        .map(|item| item.name.clone())
        .collect::<std::collections::HashSet<_>>();

    for item in &mut definitions.item_definitions {
        normalize_item_type_refs(&mut item.type_, &custom_types);
    }
    for input in &mut definitions.inputs {
        normalize_optional_type_ref(&mut input.type_ref, &custom_types);
    }
    for decision in &mut definitions.decisions {
        normalize_optional_type_ref(&mut decision.type_ref, &custom_types);
        normalize_expression_type_refs(&mut decision.logic, &custom_types);
    }
    for bkm in &mut definitions.bkms {
        normalize_optional_type_ref(&mut bkm.type_ref, &custom_types);
        for parameter in &mut bkm.parameters {
            normalize_type_ref(&mut parameter.type_ref, &custom_types);
        }
        normalize_expression_type_refs(&mut bkm.logic, &custom_types);
    }
}

/// Keep information requirements aligned with the referenced element kind.
///
/// LLMs occasionally put a decision name in `requiredInputs` (or an input
/// name in `requiredDecisions`). Since top-level names are globally unique,
/// these mistakes can be corrected without guessing user intent.
pub fn normalize_dependency_references(definitions: &mut Definitions) {
    let input_names = definitions
        .inputs
        .iter()
        .map(|input| input.name.clone())
        .collect::<std::collections::HashSet<_>>();
    let decision_names = definitions
        .decisions
        .iter()
        .map(|decision| decision.name.clone())
        .collect::<std::collections::HashSet<_>>();

    for decision in &mut definitions.decisions {
        let mut required_inputs = Vec::new();
        let mut required_decisions = Vec::new();
        for reference in decision.required_inputs.drain(..) {
            if decision_names.contains(&reference) {
                required_decisions.push(reference);
            } else {
                required_inputs.push(reference);
            }
        }
        for reference in decision.required_decisions.drain(..) {
            if input_names.contains(&reference) {
                required_inputs.push(reference);
            } else {
                required_decisions.push(reference);
            }
        }
        deduplicate_references(&mut required_inputs);
        deduplicate_references(&mut required_decisions);
        decision.required_inputs = required_inputs;
        decision.required_decisions = required_decisions;
    }
}

fn deduplicate_references(references: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    references.retain(|reference| seen.insert(reference.clone()));
}

fn normalize_optional_type_ref(
    type_ref: &mut Option<String>,
    custom_types: &std::collections::HashSet<String>,
) {
    if let Some(type_ref) = type_ref {
        normalize_type_ref(type_ref, custom_types);
    }
}

fn normalize_type_ref(type_ref: &mut String, custom_types: &std::collections::HashSet<String>) {
    if custom_types.contains(type_ref.as_str()) {
        return;
    }
    let normalized = match type_ref.as_str() {
        "integer" | "int" | "long" | "double" | "float" | "decimal" => "number",
        "feel:integer" | "feel:int" | "feel:long" | "feel:double" | "feel:float"
        | "feel:decimal" => "feel:number",
        _ => return,
    };
    *type_ref = normalized.to_string();
}

fn normalize_item_type_refs(
    item_type: &mut ItemType,
    custom_types: &std::collections::HashSet<String>,
) {
    match item_type {
        ItemType::Simple { type_ref } | ItemType::Enumeration { type_ref, .. } => {
            normalize_type_ref(type_ref, custom_types)
        }
        ItemType::Structure { fields } => {
            for field in fields {
                normalize_type_ref(&mut field.type_, custom_types);
            }
        }
        ItemType::List { item_type } => normalize_item_type_refs(item_type, custom_types),
    }
}

fn normalize_expression_type_refs(
    expression: &mut BoxedExpression,
    custom_types: &std::collections::HashSet<String>,
) {
    match expression {
        BoxedExpression::DecisionTable {
            inputs, outputs, ..
        } => {
            for input in inputs {
                normalize_optional_type_ref(&mut input.type_ref, custom_types);
            }
            for output in outputs {
                normalize_type_ref(&mut output.type_ref, custom_types);
            }
        }
        BoxedExpression::Context { entries, .. } => {
            for entry in entries {
                normalize_expression_type_refs(&mut entry.value, custom_types);
            }
        }
        BoxedExpression::FunctionDefinition { parameters, body } => {
            for parameter in parameters {
                normalize_type_ref(&mut parameter.type_ref, custom_types);
            }
            normalize_expression_type_refs(body, custom_types);
        }
        BoxedExpression::LiteralExpression { type_ref, .. } => {
            normalize_optional_type_ref(type_ref, custom_types);
        }
        BoxedExpression::Conditional { then, else_, .. } => {
            normalize_expression_type_refs(then, custom_types);
            normalize_expression_type_refs(else_, custom_types);
        }
        BoxedExpression::Invocation { .. }
        | BoxedExpression::List { .. }
        | BoxedExpression::Relation { .. }
        | BoxedExpression::Filter { .. }
        | BoxedExpression::For { .. }
        | BoxedExpression::Quantified { .. } => {}
    }
}

/// 校验 DSL 完整性
/// 校验 DSL 完整性与业务逻辑
/// 校验 DSL 完整性、业务逻辑与 DAG 依赖
pub fn validate(d: &Definitions) -> anyhow::Result<()> {
    if d.name.trim().is_empty() {
        anyhow::bail!("definitions.name 不能为空");
    }

    // Every named DMN element is used as a FEEL reference and as an XML ID-map
    // key. Duplicate or blank names would silently overwrite those mappings.
    let mut names = std::collections::HashSet::<String>::new();
    let mut check_name = |kind: &str, name: &str| -> anyhow::Result<()> {
        if name.trim().is_empty() {
            anyhow::bail!("{} 名称不能为空", kind);
        }
        if !names.insert(name.to_string()) {
            anyhow::bail!("DMN 元素名称重复: '{}'", name);
        }
        Ok(())
    };
    for item in &d.item_definitions {
        check_name("itemDefinition", &item.name)?;
    }
    for input in &d.inputs {
        check_name("input", &input.name)?;
    }
    for decision in &d.decisions {
        check_name("decision", &decision.name)?;
    }
    for bkm in &d.bkms {
        check_name("bkm", &bkm.name)?;
    }
    for service in &d.decision_services {
        check_name("decisionService", &service.name)?;
    }
    for source in &d.knowledge_sources {
        check_name("knowledgeSource", &source.name)?;
    }

    let item_definition_names: std::collections::HashSet<_> = d
        .item_definitions
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    let input_names: Vec<_> = d.inputs.iter().map(|i| &i.name).collect();
    let decision_names: Vec<_> = d.decisions.iter().map(|x| &x.name).collect();
    let bkm_names: Vec<_> = d.bkms.iter().map(|x| &x.name).collect();

    for item in &d.item_definitions {
        validate_item_type(
            &item.type_,
            &format!("itemDefinition '{}'", item.name),
            &item_definition_names,
        )?;
    }
    for input in &d.inputs {
        if let Some(type_ref) = &input.type_ref {
            validate_type_ref(
                &format!("input '{}'", input.name),
                type_ref,
                &item_definition_names,
            )?;
        }
    }
    for decision in &d.decisions {
        if let Some(type_ref) = &decision.type_ref {
            validate_type_ref(
                &format!("decision '{}'", decision.name),
                type_ref,
                &item_definition_names,
            )?;
        }
        validate_expression_type_refs(
            &decision.logic,
            &format!("decision '{}'", decision.name),
            &item_definition_names,
        )?;
    }
    for bkm in &d.bkms {
        if let Some(type_ref) = &bkm.type_ref {
            validate_type_ref(
                &format!("bkm '{}'", bkm.name),
                type_ref,
                &item_definition_names,
            )?;
        }
        validate_expression_type_refs(
            &bkm.logic,
            &format!("bkm '{}'", bkm.name),
            &item_definition_names,
        )?;
    }
    for service in &d.decision_services {
        for decision in service
            .output_decisions
            .iter()
            .chain(service.encapsulated_decisions.iter())
        {
            if !decision_names.contains(&decision) {
                anyhow::bail!(
                    "decisionService '{}' 引用了不存在的 decision: '{}'",
                    service.name,
                    decision
                );
            }
        }
        for input in &service.input_data {
            if !input_names.contains(&input) {
                anyhow::bail!(
                    "decisionService '{}' 引用了不存在的 input: '{}'",
                    service.name,
                    input
                );
            }
        }
    }
    for source in &d.knowledge_sources {
        for target in &source.authority_targets {
            if !decision_names.contains(&target) {
                anyhow::bail!(
                    "knowledgeSource '{}' 引用了不存在的 decision: '{}'",
                    source.name,
                    target
                );
            }
        }
    }
    for annotation in &d.text_annotations {
        if !names.contains(&annotation.target) {
            anyhow::bail!("textAnnotation 引用了不存在的元素: '{}'", annotation.target);
        }
    }

    let mut graph = DiGraph::<&String, ()>::new();
    let mut node_map = std::collections::HashMap::new();

    for dec in &d.decisions {
        // 1. 基础引用校验
        for ri in &dec.required_inputs {
            if !input_names.contains(&ri) {
                anyhow::bail!("决策 '{}' 引用了不存在的 input: '{}'", dec.name, ri);
            }
        }
        for rk in &dec.required_knowledge {
            if !bkm_names.contains(&rk) {
                anyhow::bail!("决策 '{}' 引用了不存在的 bkm: '{}'", dec.name, rk);
            }
        }
        let n = graph.add_node(&dec.name);
        node_map.insert(&dec.name, n);
    }

    if let Some(diagram) = &d.diagram {
        let relationships: std::collections::HashSet<(&str, &str)> = d
            .decisions
            .iter()
            .flat_map(|decision| {
                decision
                    .required_inputs
                    .iter()
                    .chain(decision.required_decisions.iter())
                    .chain(decision.required_knowledge.iter())
                    .map(move |source| (source.as_str(), decision.name.as_str()))
            })
            .collect();
        for node in &diagram.nodes {
            if !names.contains(node.element.as_str()) {
                anyhow::bail!("图节点引用了不存在的元素: '{}'", node.element);
            }
            if !node.x.is_finite()
                || !node.y.is_finite()
                || node.width.is_some_and(|v| !v.is_finite() || v <= 0.0)
                || node.height.is_some_and(|v| !v.is_finite() || v <= 0.0)
            {
                anyhow::bail!("图节点 '{}' 的坐标或尺寸无效", node.element);
            }
        }
        for edge in &diagram.edges {
            if !names.contains(edge.source.as_str()) || !names.contains(edge.target.as_str()) {
                anyhow::bail!(
                    "图连线引用了不存在的元素: '{}' -> '{}'",
                    edge.source,
                    edge.target
                );
            }
            if !relationships.contains(&(edge.source.as_str(), edge.target.as_str())) {
                anyhow::bail!(
                    "图连线 '{}' -> '{}' 不对应任何决策依赖关系",
                    edge.source,
                    edge.target
                );
            }
            if edge
                .waypoints
                .iter()
                .any(|(x, y)| !x.is_finite() || !y.is_finite())
            {
                anyhow::bail!("图连线 '{}' -> '{}' 包含无效坐标", edge.source, edge.target);
            }
        }
    }

    for dec in &d.decisions {
        for rd in &dec.required_decisions {
            if !decision_names.contains(&rd) {
                anyhow::bail!("决策 '{}' 引用了不存在的 decision: '{}'", dec.name, rd);
            }
            if let (Some(&u), Some(&v)) = (node_map.get(&rd), node_map.get(&dec.name)) {
                graph.add_edge(u, v, ());
            }
        }
    }

    // 2. 循环依赖检测 (精准报错)
    if toposort(&graph, None).is_err() {
        // 提取环中的节点用于报错
        let cycle_nodes: Vec<String> = graph
            .node_indices()
            .filter_map(|n| {
                let name = graph[n].to_string();
                if graph
                    .neighbors_directed(n, petgraph::Direction::Incoming)
                    .any(|_| true)
                {
                    Some(name)
                } else {
                    None
                }
            })
            .collect();
        anyhow::bail!(
            "检测到决策之间存在循环依赖 (Deadlock)！涉及节点: [{}]",
            cycle_nodes.join(", ")
        );
    }

    // 3. 决策表深度精准校验
    for dec in &d.decisions {
        if let BoxedExpression::DecisionTable {
            hit_policy,
            aggregation,
            inputs,
            outputs,
            rules,
            ..
        } = &dec.logic
        {
            if let Some(hp) = hit_policy
                && hp == "COLLECT"
                && aggregation.is_none()
            {
                anyhow::bail!(
                    "决策 '{}' 的命中策略为 COLLECT，但未指定 aggregation",
                    dec.name
                );
            }

            for (i, rule) in rules.iter().enumerate() {
                if rule.input_entries.len() != inputs.len() {
                    anyhow::bail!(
                        "决策 '{}' 第 {} 条规则: 输入条目数 ({}) 与输入列数 ({}) 不匹配",
                        dec.name,
                        i + 1,
                        rule.input_entries.len(),
                        inputs.len()
                    );
                }
                if rule.output_entries.len() != outputs.len() {
                    anyhow::bail!(
                        "决策 '{}' 第 {} 条规则: 输出条目数 ({}) 与输出列数 ({}) 不匹配",
                        dec.name,
                        i + 1,
                        rule.output_entries.len(),
                        outputs.len()
                    );
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_internal_tag_expression() {
        let j = r#"{"type": "literalExpression", "expression": "a+b", "typeRef": "number"}"#;
        let e: BoxedExpression = serde_json::from_str(j).unwrap();
        assert!(matches!(e, BoxedExpression::LiteralExpression { .. }));
    }

    #[test]
    fn parse_internal_tag_text_alias() {
        let j = r#"{"type": "literalExpression", "text": "a+b", "typeRef": "number"}"#;
        let e: BoxedExpression = serde_json::from_str(j).unwrap();
        assert!(
            matches!(e, BoxedExpression::LiteralExpression { expression, .. } if expression == "a+b")
        );
    }

    #[test]
    fn parse_external_tag() {
        let j = r#"{"literalExpression": {"text": "a+b", "typeRef": "number"}}"#;
        let e: BoxedExpression = serde_json::from_str(j).unwrap();
        assert!(
            matches!(e, BoxedExpression::LiteralExpression { expression, .. } if expression == "a+b")
        );
    }

    #[test]
    fn parse_external_tag_nested_context() {
        let j = r#"{"context": {"entries": [{"key": "tmp", "value": {"literalExpression": {"text": "1+1"}}}]}}"#;
        let e: BoxedExpression = serde_json::from_str(j).unwrap();
        assert!(matches!(e, BoxedExpression::Context { .. }));
    }

    #[test]
    fn context_entry_accepts_name_alias() {
        let j = r#"{"context": {"entries": [{"name": "tmp", "value": {"type": "literalExpression", "expression": "1+1"}}]}}"#;
        let e: BoxedExpression = serde_json::from_str(j).unwrap();
        assert!(matches!(
            e,
            BoxedExpression::Context { entries, .. }
                if entries[0].key == "tmp"
        ));
    }

    #[test]
    fn parse_internal_tag_value_alias() {
        let j = r#"{"type": "literalExpression", "value": "a+b", "typeRef": "number"}"#;
        let e: BoxedExpression = serde_json::from_str(j).unwrap();
        assert!(
            matches!(e, BoxedExpression::LiteralExpression { expression, .. } if expression == "a+b")
        );
    }

    #[test]
    fn parse_external_tag_value_alias() {
        let j = r#"{"literalExpression": {"value": "a+b"}}"#;
        let e: BoxedExpression = serde_json::from_str(j).unwrap();
        assert!(
            matches!(e, BoxedExpression::LiteralExpression { expression, .. } if expression == "a+b")
        );
    }

    #[test]
    fn parse_external_tag_decision_table() {
        let j = r#"{"decisionTable": {"hitPolicy": "UNIQUE", "inputs": [{"expression": "a"}], "outputs": [{"name": "r", "typeRef": "string"}], "rules": [{"inputEntries": ["1"], "outputEntries": ["\"x\""]}]}}"#;
        let e: BoxedExpression = serde_json::from_str(j).unwrap();
        assert!(matches!(e, BoxedExpression::DecisionTable { .. }));
    }

    #[test]
    fn parse_yaml_internal_tag() {
        let y = "type: literalExpression\nexpression: \"a+b\"\ntypeRef: number\n";
        let e: BoxedExpression = serde_yaml::from_str(y).unwrap();
        assert!(matches!(e, BoxedExpression::LiteralExpression { .. }));
    }

    #[test]
    fn serialize_roundtrip() {
        let e = BoxedExpression::LiteralExpression {
            expression: "a+b".into(),
            type_ref: Some("number".into()),
        };
        let j = serde_json::to_string(&e).unwrap();
        assert!(j.contains(r#""type":"literalExpression""#));
        let back: BoxedExpression = serde_json::from_str(&j).unwrap();
        assert!(matches!(back, BoxedExpression::LiteralExpression { .. }));
    }

    // ---- ItemType 宽容反序列化 ----

    #[test]
    fn parse_item_type_internal_structure() {
        // 标准 internal tag
        let j = r#"{"kind": "structure", "fields": [{"name": "age", "type": "number"}]}"#;
        let t: ItemType = serde_json::from_str(j).unwrap();
        assert!(matches!(t, ItemType::Structure { .. }));
    }

    #[test]
    fn parse_item_type_external_structure_object() {
        // external tag：{"structure": {"fields": [...]}}
        let j = r#"{"structure": {"fields": [{"name": "age", "type": "number"}]}}"#;
        let t: ItemType = serde_json::from_str(j).unwrap();
        assert!(matches!(t, ItemType::Structure { .. }));
    }

    #[test]
    fn parse_item_type_external_structure_array() {
        // LLM 实测输出：{"structure": [{"name": "age", "typeRef": "number"}]}
        // 值直接是字段数组 + 字段用 typeRef 别名
        let j = r#"{"structure": [{"name": "age", "typeRef": "number"}, {"name": "gender", "typeRef": "string"}]}"#;
        let t: ItemType = serde_json::from_str(j).unwrap();
        match t {
            ItemType::Structure { fields } => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name, "age");
                assert_eq!(fields[0].type_, "number");
            }
            _ => panic!("应解析为 Structure"),
        }
    }

    #[test]
    fn parse_item_type_external_simple_string() {
        // external tag 缩写：{"simple": "number"}
        let j = r#"{"simple": "number"}"#;
        let t: ItemType = serde_json::from_str(j).unwrap();
        assert!(matches!(t, ItemType::Simple { type_ref } if type_ref == "number"));
    }

    #[test]
    fn parse_item_type_external_simple_object() {
        // external tag：{"simple": {"typeRef": "number"}}
        let j = r#"{"simple": {"typeRef": "number"}}"#;
        let t: ItemType = serde_json::from_str(j).unwrap();
        assert!(matches!(t, ItemType::Simple { type_ref } if type_ref == "number"));
    }

    #[test]
    fn parse_item_type_external_list_nested() {
        // external list 嵌套：{"list": {"itemType": {"simple": "string"}}}
        let j = r#"{"list": {"itemType": {"simple": "string"}}}"#;
        let t: ItemType = serde_json::from_str(j).unwrap();
        assert!(matches!(t, ItemType::List { .. }));
    }

    #[test]
    fn parse_item_type_external_enumeration() {
        let j = r#"{"enumeration": {"typeRef": "string", "values": ["A", "B"]}}"#;
        let t: ItemType = serde_json::from_str(j).unwrap();
        assert!(matches!(t, ItemType::Enumeration { values, .. } if values == vec!["A", "B"]));
    }

    #[test]
    fn parse_item_type_invalid_format() {
        // 既不是 internal 也不是 external 的格式应报错
        let j = r#"{"foo": "bar", "baz": 1}"#;
        assert!(serde_json::from_str::<ItemType>(j).is_err());
    }

    #[test]
    fn validate_rejects_duplicate_element_names() {
        let definitions = parse_definitions(
            r#"
name: duplicate-names
inputs:
  - name: applicant
    typeRef: string
decisions:
  - name: applicant
    logic:
      type: literalExpression
      expression: applicant
"#,
        )
        .unwrap();

        let error = validate(&definitions).unwrap_err().to_string();
        assert!(error.contains("名称重复"));
    }

    #[test]
    fn validate_rejects_unknown_type_references() {
        let definitions = parse_definitions(
            r#"
name: UnknownTypeModel
inputs:
  - name: Applicant
    typeRef: MissingApplicant
decisions:
  - name: Approval
    typeRef: string
    logic:
      type: literalExpression
      expression: '"Approve"'
"#,
        )
        .unwrap();

        let error = validate(&definitions).unwrap_err().to_string();
        assert!(error.contains("input 'Applicant' 引用了未定义的 typeRef"));
    }

    #[test]
    fn normalize_common_numeric_type_aliases_to_number() {
        let definitions = parse_definitions(
            r#"
name: NumericAliases
inputs:
  - name: Orders
    typeRef: integer
  - name: Ratio
    typeRef: feel:double
decisions:
  - name: Score
    typeRef: long
    requiredInputs: [Orders]
    logic:
      type: literalExpression
      expression: Orders
      typeRef: decimal
"#,
        )
        .unwrap();

        assert_eq!(definitions.inputs[0].type_ref.as_deref(), Some("number"));
        assert_eq!(
            definitions.inputs[1].type_ref.as_deref(),
            Some("feel:number")
        );
        assert_eq!(definitions.decisions[0].type_ref.as_deref(), Some("number"));
        assert!(matches!(
            definitions.decisions[0].logic,
            BoxedExpression::LiteralExpression { type_ref: Some(ref type_ref), .. }
                if type_ref == "number"
        ));
        validate(&definitions).unwrap();
    }

    #[test]
    fn normalize_numeric_alias_preserves_custom_item_definition_name() {
        let definitions = parse_definitions(
            r#"
name: CustomInteger
itemDefinitions:
  - name: integer
    type:
      kind: simple
      typeRef: string
inputs:
  - name: Value
    typeRef: integer
decisions: []
"#,
        )
        .unwrap();

        assert_eq!(definitions.inputs[0].type_ref.as_deref(), Some("integer"));
        validate(&definitions).unwrap();
    }

    #[test]
    fn normalize_dependency_references_moves_misclassified_names() {
        let definitions = parse_definitions(
            r#"
name: DependencyAliases
inputs:
  - name: Orders
    typeRef: number
decisions:
  - name: ShippingRatio
    typeRef: number
    requiredInputs: [Orders]
    logic:
      type: literalExpression
      expression: Orders
  - name: RiskLevel
    typeRef: string
    requiredInputs: [ShippingRatio]
    requiredDecisions: [Orders]
    logic:
      type: literalExpression
      expression: '"Low"'
"#,
        )
        .unwrap();

        let risk = &definitions.decisions[1];
        assert_eq!(risk.required_inputs, vec!["Orders"]);
        assert_eq!(risk.required_decisions, vec!["ShippingRatio"]);
        validate(&definitions).unwrap();
    }

    #[test]
    fn validate_rejects_invalid_decision_service_references() {
        let definitions = parse_definitions(
            r#"
name: ServiceModel
inputs:
  - name: Applicant
    typeRef: string
decisions:
  - name: Approval
    typeRef: string
    logic:
      type: literalExpression
      expression: Applicant
decisionServices:
  - name: ApprovalService
    outputDecisions: [MissingDecision]
    inputData: [Applicant]
"#,
        )
        .unwrap();

        let error = validate(&definitions).unwrap_err().to_string();
        assert!(error.contains("decisionService 'ApprovalService' 引用了不存在的 decision"));
    }

    #[test]
    fn validate_accepts_user_defined_type_references() {
        let definitions = parse_definitions(
            r#"
name: TypedModel
itemDefinitions:
  - name: ApplicantType
    type:
      kind: structure
      fields:
        - name: age
          type: number
inputs:
  - name: Applicant
    typeRef: ApplicantType
decisions:
  - name: AdultCheck
    typeRef: boolean
    requiredInputs: [Applicant]
    logic:
      type: literalExpression
      expression: Applicant.age >= 18
      typeRef: boolean
"#,
        )
        .unwrap();

        validate(&definitions).unwrap();
    }

    #[test]
    fn parse_definitions_accepts_wrapped_llm_decision_table_dsl() {
        let definitions = parse_definitions(
            r#"
definitions:
  name: WrappedModel
  namespace: https://example.com/dmn/wrapped
  inputs:
    - name: score
      typeRef: number
  decisions:
    - name: Rating
      typeRef: string
      requiredInputs: [score]
      logic:
        decisionTable:
          hitPolicy: FIRST
          inputs:
            - expression: score
              typeRef: number
          outputs:
            - name: result
              typeRef: string
          rules:
            - inputEntries: [">= 700"]
              outputEntries: ['"high"']
            - inputEntries: ["-"]
              outputEntries: ['"other"']
"#,
        )
        .unwrap();

        assert_eq!(definitions.name, "WrappedModel");
        assert!(matches!(
            definitions.decisions[0].logic,
            BoxedExpression::DecisionTable { hit_policy: Some(ref policy), .. }
                if policy == "FIRST"
        ));
        validate(&definitions).unwrap();
    }

    #[test]
    fn validate_rejects_unknown_diagram_references() {
        let definitions = parse_definitions(
            r#"
name: diagram-reference
inputs:
  - name: applicant
    typeRef: string
decisions:
  - name: approved
    logic:
      type: literalExpression
      expression: applicant = "yes"
diagram:
  nodes:
    - element: missing
      x: 0
      y: 0
"#,
        )
        .unwrap();

        let error = validate(&definitions).unwrap_err().to_string();
        assert!(error.contains("不存在的元素"));
    }

    #[test]
    fn diagram_node_accepts_name_alias() {
        let node: DiagramNode =
            serde_json::from_str(r#"{"name":"Applicant","x":0,"y":0}"#).unwrap();
        assert_eq!(node.element, "Applicant");
    }
}
