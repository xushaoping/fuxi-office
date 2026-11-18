use super::error::{DmnError, Result};
use super::model::*;
use quick_xml::Reader;
use quick_xml::events::Event;
use std::collections::{HashMap, HashSet};

const FUXI_TRACE_NS: &str = "urn:fuxi:dmn:traceability:v1";

#[derive(Debug, Clone)]
struct XmlNode {
    local: String,
    namespace: String,
    attrs: HashMap<String, String>,
    text: String,
    children: Vec<XmlNode>,
}

fn local_name(name: &[u8]) -> String {
    let name = String::from_utf8_lossy(name);
    name.rsplit(':').next().unwrap_or_default().to_owned()
}

fn prefix(name: &[u8]) -> String {
    let name = String::from_utf8_lossy(name);
    name.split_once(':')
        .map(|(prefix, _)| prefix.to_owned())
        .unwrap_or_default()
}

fn attrs_and_namespaces(
    e: &quick_xml::events::BytesStart,
    inherited: &HashMap<String, String>,
) -> Result<(HashMap<String, String>, HashMap<String, String>)> {
    let mut namespaces = inherited.clone();
    let mut attrs = HashMap::new();
    for attribute in e.attributes() {
        let attribute = attribute.map_err(|error| DmnError::Xml(error.to_string()))?;
        let key = String::from_utf8_lossy(attribute.key.as_ref()).to_string();
        let value = attribute
            .unescape_value()
            .map_err(|error| DmnError::Xml(error.to_string()))?
            .into_owned();
        if key == "xmlns" {
            namespaces.insert(String::new(), value);
        } else if let Some(prefix) = key.strip_prefix("xmlns:") {
            namespaces.insert(prefix.to_owned(), value);
        } else {
            attrs.insert(key.rsplit(':').next().unwrap_or(&key).to_owned(), value);
        }
    }
    Ok((attrs, namespaces))
}

fn parse_xml(xml: &str) -> Result<XmlNode> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut stack: Vec<(XmlNode, HashMap<String, String>)> = Vec::new();
    let mut root = None;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let inherited = stack
                    .last()
                    .map(|(_, namespaces)| namespaces)
                    .cloned()
                    .unwrap_or_default();
                let (attrs, namespaces) = attrs_and_namespaces(&e, &inherited)?;
                let node = XmlNode {
                    local: local_name(e.name().as_ref()),
                    namespace: namespaces
                        .get(&prefix(e.name().as_ref()))
                        .cloned()
                        .unwrap_or_default(),
                    attrs,
                    text: String::new(),
                    children: Vec::new(),
                };
                stack.push((node, namespaces));
            }
            Ok(Event::Empty(e)) => {
                let inherited = stack
                    .last()
                    .map(|(_, namespaces)| namespaces)
                    .cloned()
                    .unwrap_or_default();
                let (attrs, namespaces) = attrs_and_namespaces(&e, &inherited)?;
                let node = XmlNode {
                    local: local_name(e.name().as_ref()),
                    namespace: namespaces
                        .get(&prefix(e.name().as_ref()))
                        .cloned()
                        .unwrap_or_default(),
                    attrs,
                    text: String::new(),
                    children: Vec::new(),
                };
                if let Some((parent, _)) = stack.last_mut() {
                    parent.children.push(node);
                } else {
                    root = Some(node);
                }
            }
            Ok(Event::End(_)) => {
                let (node, _) = stack
                    .pop()
                    .ok_or_else(|| DmnError::Xml("unbalanced XML close tag".into()))?;
                if let Some((parent, _)) = stack.last_mut() {
                    parent.children.push(node);
                } else {
                    root = Some(node);
                }
            }
            Ok(Event::Text(text)) => {
                if let Some((node, _)) = stack.last_mut() {
                    node.text.push_str(
                        &text
                            .unescape()
                            .map_err(|error| DmnError::Xml(error.to_string()))?,
                    );
                }
            }
            Ok(Event::CData(text)) => {
                if let Some((node, _)) = stack.last_mut() {
                    node.text.push_str(&String::from_utf8_lossy(&text));
                }
            }
            Ok(_) => {}
            Err(error) => return Err(DmnError::Xml(error.to_string())),
        }
        buf.clear();
    }
    root.ok_or_else(|| DmnError::Xml("empty document".into()))
}

fn dmn_children<'a>(node: &'a XmlNode, local: &str) -> impl Iterator<Item = &'a XmlNode> {
    node.children
        .iter()
        .filter(move |child| child.local == local && is_supported_dmn_namespace(&child.namespace))
}
fn dmn_child<'a>(node: &'a XmlNode, local: &str) -> Option<&'a XmlNode> {
    dmn_children(node, local).next()
}
fn required(node: &XmlNode, attribute: &str) -> Result<String> {
    node.attrs
        .get(attribute)
        .cloned()
        .ok_or_else(|| DmnError::MissingAttribute {
            element: node.local.clone(),
            attribute: attribute.to_owned(),
        })
}
fn text(node: &XmlNode) -> String {
    node.text.trim().to_owned()
}
fn reference(node: &XmlNode) -> Result<String> {
    required(node, "href").map(|href| href.strip_prefix('#').unwrap_or(&href).to_owned())
}

fn requirement_ids(node: &XmlNode) -> Vec<String> {
    node.children
        .iter()
        .filter(|child| child.local == "extensionElements")
        .flat_map(|extensions| extensions.children.iter())
        .filter(|trace| trace.local == "traceability" && trace.namespace == FUXI_TRACE_NS)
        .flat_map(|trace| trace.children.iter())
        .filter(|reference| {
            reference.local == "requirementRef" && reference.namespace == FUXI_TRACE_NS
        })
        .filter_map(|reference| reference.attrs.get("id").cloned())
        .collect()
}

pub fn parse_dmn(xml: &str) -> Result<DmnModel> {
    let root = parse_xml(xml)?;
    if root.local != "definitions" || !is_supported_dmn_namespace(&root.namespace) {
        return Err(DmnError::Xml(
            "document root must be a supported DMN definitions element".into(),
        ));
    }
    let mut ids = HashSet::new();
    let mut item_definitions = Vec::new();
    let mut input_data = Vec::new();
    let mut decisions = Vec::new();
    let mut business_knowledge_models = Vec::new();
    let mut imports = Vec::new();
    let mut decision_services = Vec::new();
    for child in &root.children {
        if !is_supported_dmn_namespace(&child.namespace) {
            continue;
        }
        match child.local.as_str() {
            "itemDefinition" => item_definitions.push(parse_item_definition(child)?),
            "inputData" => {
                let id = required(child, "id")?;
                if !ids.insert(id.clone()) {
                    return Err(DmnError::DuplicateId(id));
                }
                let variable_node = dmn_child(child, "variable");
                let variable = variable_node
                    .and_then(|variable| variable.attrs.get("name"))
                    .cloned();
                input_data.push(InputData {
                    name: child
                        .attrs
                        .get("name")
                        .cloned()
                        .unwrap_or_else(|| id.clone()),
                    id,
                    variable,
                    type_ref: variable_node
                        .and_then(|variable| variable.attrs.get("typeRef"))
                        .cloned(),
                });
            }
            "decision" => decisions.push(parse_decision(child, &mut ids)?),
            "businessKnowledgeModel" => {
                business_knowledge_models.push(parse_business_knowledge_model(child, &mut ids)?)
            }
            "import" => imports.push(Import {
                id: child.attrs.get("id").cloned(),
                name: child.attrs.get("name").cloned(),
                namespace: child.attrs.get("namespace").cloned(),
                location_uri: child.attrs.get("locationURI").cloned(),
            }),
            "decisionService" => {
                let id = required(child, "id")?;
                if !ids.insert(id.clone()) {
                    return Err(DmnError::DuplicateId(id));
                }
                decision_services.push(DecisionService {
                    name: child
                        .attrs
                        .get("name")
                        .cloned()
                        .unwrap_or_else(|| id.clone()),
                    id,
                    output_decisions: dmn_children(child, "outputDecision")
                        .map(reference)
                        .collect::<Result<Vec<_>>>()?,
                    encapsulated_decisions: dmn_children(child, "encapsulatedDecision")
                        .map(reference)
                        .collect::<Result<Vec<_>>>()?,
                    input_decisions: dmn_children(child, "inputDecision")
                        .map(reference)
                        .collect::<Result<Vec<_>>>()?,
                    input_data: dmn_children(child, "inputData")
                        .map(reference)
                        .collect::<Result<Vec<_>>>()?,
                });
            }
            _ => {}
        }
    }
    Ok(DmnModel {
        name: root.attrs.get("name").cloned(),
        namespace: root.attrs.get("namespace").cloned(),
        expression_language: root.attrs.get("expressionLanguage").cloned(),
        type_language: root.attrs.get("typeLanguage").cloned(),
        input_data,
        item_definitions,
        decisions,
        business_knowledge_models,
        imports,
        decision_services,
    })
}

fn parse_item_definition(node: &XmlNode) -> Result<ItemDefinition> {
    let type_ref = dmn_child(node, "typeRef")
        .map(text)
        .filter(|value| !value.is_empty());
    let allowed_values = dmn_child(node, "allowedValues")
        .and_then(|values| dmn_child(values, "text"))
        .map(text)
        .filter(|value| !value.is_empty());
    Ok(ItemDefinition {
        id: node.attrs.get("id").cloned(),
        name: required(node, "name")?,
        type_ref,
        is_collection: node
            .attrs
            .get("isCollection")
            .is_some_and(|value| value == "true"),
        allowed_values,
        item_components: dmn_children(node, "itemComponent")
            .map(parse_item_definition)
            .collect::<Result<Vec<_>>>()?,
    })
}

fn parse_decision(node: &XmlNode, ids: &mut HashSet<String>) -> Result<Decision> {
    let id = required(node, "id")?;
    if !ids.insert(id.clone()) {
        return Err(DmnError::DuplicateId(id));
    }
    let name = node
        .attrs
        .get("name")
        .cloned()
        .unwrap_or_else(|| id.clone());
    let variable_node = dmn_child(node, "variable");
    let variable = variable_node
        .and_then(|variable| variable.attrs.get("name"))
        .cloned();
    let mut requirements = Vec::new();
    for requirement in dmn_children(node, "informationRequirement") {
        let required_input = dmn_child(requirement, "requiredInput")
            .map(reference)
            .transpose()?;
        let required_decision = dmn_child(requirement, "requiredDecision")
            .map(reference)
            .transpose()?;
        requirements.push(InformationRequirement {
            id: requirement.attrs.get("id").cloned(),
            required_input,
            required_decision,
        });
    }
    let logic = parse_expression(node)?;
    Ok(Decision {
        id,
        name,
        variable,
        type_ref: variable_node
            .and_then(|variable| variable.attrs.get("typeRef"))
            .cloned(),
        requirements,
        knowledge_requirements: parse_knowledge_requirements(node)?,
        requirement_ids: requirement_ids(node),
        logic,
    })
}

fn parse_business_knowledge_model(
    node: &XmlNode,
    ids: &mut HashSet<String>,
) -> Result<BusinessKnowledgeModel> {
    let id = required(node, "id")?;
    if !ids.insert(id.clone()) {
        return Err(DmnError::DuplicateId(id));
    }
    let variable_node = dmn_child(node, "variable");
    Ok(BusinessKnowledgeModel {
        name: node
            .attrs
            .get("name")
            .cloned()
            .unwrap_or_else(|| id.clone()),
        variable: variable_node
            .and_then(|variable| variable.attrs.get("name"))
            .cloned(),
        type_ref: variable_node
            .and_then(|variable| variable.attrs.get("typeRef"))
            .cloned(),
        knowledge_requirements: parse_knowledge_requirements(node)?,
        logic: match dmn_child(node, "encapsulatedLogic") {
            Some(logic) => parse_encapsulated_logic(logic)?,
            None => parse_expression(node)?,
        },
        id,
    })
}

fn parse_knowledge_requirements(node: &XmlNode) -> Result<Vec<KnowledgeRequirement>> {
    dmn_children(node, "knowledgeRequirement")
        .map(|requirement| {
            let required_knowledge = dmn_child(requirement, "requiredKnowledge")
                .ok_or_else(|| {
                    DmnError::Xml("knowledgeRequirement is missing requiredKnowledge".into())
                })
                .and_then(reference)?;
            Ok(KnowledgeRequirement {
                id: requirement.attrs.get("id").cloned(),
                required_knowledge,
            })
        })
        .collect()
}

fn parse_encapsulated_logic(node: &XmlNode) -> Result<DecisionLogic> {
    let (parameters, parameter_type_refs) = parse_formal_parameters(node)?;
    let body = expression_child(node)
        .ok_or_else(|| DmnError::Xml("encapsulatedLogic is missing expression".into()))?;
    Ok(DecisionLogic::FunctionDefinition(FunctionDefinition {
        parameters,
        parameter_type_refs,
        kind: node.attrs.get("kind").cloned(),
        result_type_ref: node.attrs.get("typeRef").cloned(),
        body: Box::new(parse_expression(body)?),
    }))
}

fn parse_formal_parameters(node: &XmlNode) -> Result<(Vec<String>, Vec<Option<String>>)> {
    let parameters = dmn_children(node, "formalParameter")
        .map(|parameter| {
            Ok((
                required(parameter, "name")?,
                parameter.attrs.get("typeRef").cloned(),
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(parameters.into_iter().unzip())
}

fn is_expression_element(local: &str) -> bool {
    matches!(
        local,
        "decisionTable"
            | "literalExpression"
            | "context"
            | "invocation"
            | "relation"
            | "list"
            | "functionDefinition"
            | "conditional"
            | "filter"
            | "for"
            | "some"
            | "every"
    )
}

fn expression_child(node: &XmlNode) -> Option<&XmlNode> {
    for child in &node.children {
        if !is_supported_dmn_namespace(&child.namespace) {
            continue;
        }
        if is_expression_element(&child.local) {
            return Some(child);
        }
        if matches!(child.local.as_str(), "expression" | "encapsulatedLogic")
            && let Some(expression) = expression_child(child)
        {
            return Some(expression);
        }
    }
    None
}

fn parse_expression(node: &XmlNode) -> Result<DecisionLogic> {
    let expression = if is_expression_element(&node.local) {
        Some(node)
    } else {
        expression_child(node)
    };
    let Some(expression) = expression else {
        return Ok(DecisionLogic::None);
    };
    ensure_feel_expression_language(expression)?;
    match expression.local.as_str() {
        "decisionTable" => Ok(DecisionLogic::DecisionTable(parse_table(expression)?)),
        "literalExpression" => Ok(DecisionLogic::LiteralExpression(
            dmn_child(expression, "text")
                .map(text)
                .unwrap_or_else(|| text(expression)),
        )),
        "context" => {
            let mut entries = Vec::new();
            for entry in dmn_children(expression, "contextEntry") {
                let variable = dmn_child(entry, "variable");
                let name = variable
                    .map(|variable| required(variable, "name"))
                    .transpose()?
                    .unwrap_or_default();
                entries.push(ContextEntry {
                    name,
                    type_ref: variable
                        .and_then(|variable| variable.attrs.get("typeRef"))
                        .cloned(),
                    expression: parse_expression(entry)?,
                });
            }
            Ok(DecisionLogic::Context(entries))
        }
        "list" => Ok(DecisionLogic::List(
            expression
                .children
                .iter()
                .filter_map(|child| {
                    if !is_supported_dmn_namespace(&child.namespace) {
                        return None;
                    }
                    if child.local == "expression" {
                        expression_child(child)
                    } else if is_expression_element(&child.local) {
                        Some(child)
                    } else {
                        None
                    }
                })
                .map(parse_expression)
                .collect::<Result<Vec<_>>>()?,
        )),
        "relation" => Ok(DecisionLogic::Relation(parse_relation(expression)?)),
        "conditional" => Ok(DecisionLogic::Conditional(Conditional {
            condition: Box::new(parse_wrapped_expression(expression, "if")?),
            then_expression: Box::new(parse_wrapped_expression(expression, "then")?),
            else_expression: Box::new(parse_wrapped_expression(expression, "else")?),
        })),
        "filter" => Ok(DecisionLogic::Filter(FilterExpression {
            input: Box::new(parse_wrapped_expression(expression, "in")?),
            matcher: Box::new(parse_wrapped_expression(expression, "match")?),
        })),
        "for" => Ok(DecisionLogic::For(IteratorExpression {
            variable: required(expression, "iteratorVariable")?,
            input: Box::new(parse_wrapped_expression(expression, "in")?),
            return_expression: Box::new(parse_wrapped_expression(expression, "return")?),
        })),
        "some" | "every" => {
            let quantified = QuantifiedExpression {
                variable: required(expression, "iteratorVariable")?,
                input: Box::new(parse_wrapped_expression(expression, "in")?),
                satisfies: Box::new(parse_wrapped_expression(expression, "satisfies")?),
            };
            Ok(if expression.local == "some" {
                DecisionLogic::Some(quantified)
            } else {
                DecisionLogic::Every(quantified)
            })
        }
        "invocation" => {
            let invoked = expression
                .children
                .iter()
                .find(|child| {
                    is_supported_dmn_namespace(&child.namespace)
                        && child.local == "literalExpression"
                })
                .or_else(|| dmn_child(expression, "expression").and_then(expression_child))
                .ok_or_else(|| DmnError::Xml("invocation is missing expression".into()))?;
            let bindings = dmn_children(expression, "binding")
                .map(|binding| {
                    let parameter = dmn_child(binding, "parameter")
                        .and_then(|parameter| parameter.attrs.get("name"))
                        .cloned()
                        .ok_or_else(|| DmnError::MissingAttribute {
                            element: "binding".into(),
                            attribute: "parameter.name".into(),
                        })?;
                    Ok(Binding {
                        parameter,
                        expression: expression_child(binding)
                            .map(parse_expression)
                            .transpose()?
                            .unwrap_or(DecisionLogic::None),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(DecisionLogic::Invocation(Invocation {
                expression: Box::new(parse_expression(invoked)?),
                bindings,
            }))
        }
        "functionDefinition" => {
            let (parameters, parameter_type_refs) = parse_formal_parameters(expression)?;
            let body = expression_child(expression)
                .ok_or_else(|| DmnError::Xml("functionDefinition is missing expression".into()))?;
            Ok(DecisionLogic::FunctionDefinition(FunctionDefinition {
                parameters,
                parameter_type_refs,
                kind: expression.attrs.get("kind").cloned(),
                result_type_ref: expression.attrs.get("typeRef").cloned(),
                body: Box::new(parse_expression(body)?),
            }))
        }
        unsupported => Ok(DecisionLogic::Unsupported(unsupported.into())),
    }
}

fn ensure_feel_expression_language(node: &XmlNode) -> Result<()> {
    if let Some(language) = node
        .attrs
        .get("expressionLanguage")
        .filter(|language| !is_feel_language(language))
    {
        return Err(DmnError::UnsupportedFeature {
            decision: node.local.clone(),
            feature: format!("expression language '{language}'"),
        });
    }
    Ok(())
}

fn parse_wrapped_expression(node: &XmlNode, wrapper: &str) -> Result<DecisionLogic> {
    let wrapper_name = wrapper;
    let wrapper = dmn_child(node, wrapper_name)
        .ok_or_else(|| DmnError::Xml(format!("{} is missing {wrapper_name}", node.local)))?;
    let expression = expression_child(wrapper)
        .ok_or_else(|| DmnError::Xml(format!("{wrapper_name} is missing expression")))?;
    parse_expression(expression)
}

fn parse_relation(node: &XmlNode) -> Result<Relation> {
    let columns = dmn_children(node, "column")
        .map(|column| {
            Ok(RelationColumn {
                id: column.attrs.get("id").cloned(),
                name: required(column, "name")?,
                type_ref: column
                    .attrs
                    .get("typeRef")
                    .cloned()
                    .or_else(|| dmn_child(column, "typeRef").map(text))
                    .filter(|value| !value.is_empty()),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let rows = dmn_children(node, "row")
        .map(|row| {
            let entries = row
                .children
                .iter()
                .filter(|child| {
                    is_supported_dmn_namespace(&child.namespace)
                        && is_expression_element(&child.local)
                })
                .map(|entry| {
                    parse_expression(entry).map(|logic| match logic {
                        DecisionLogic::LiteralExpression(expression)
                            if expression.trim().is_empty() =>
                        {
                            DecisionLogic::LiteralExpression("null".into())
                        }
                        other => other,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(RelationRow {
                id: row.attrs.get("id").cloned(),
                entries,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Relation {
        id: node.attrs.get("id").cloned(),
        columns,
        rows,
    })
}

fn parse_table(node: &XmlNode) -> Result<DecisionTable> {
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    let mut rules = Vec::new();
    for child in &node.children {
        if !is_supported_dmn_namespace(&child.namespace) {
            continue;
        }
        match child.local.as_str() {
            "input" => {
                let input_expression = dmn_child(child, "inputExpression");
                if let Some(input_expression) = input_expression {
                    ensure_feel_expression_language(input_expression)?;
                }
                if let Some(input_values) = dmn_child(child, "inputValues") {
                    ensure_feel_expression_language(input_values)?;
                }
                let expression = input_expression
                    .and_then(|expression| dmn_child(expression, "text"))
                    .map(text)
                    .unwrap_or_default();
                inputs.push(InputClause {
                    expression,
                    variable: input_expression
                        .and_then(|expression| expression.attrs.get("name"))
                        .cloned(),
                    type_ref: input_expression
                        .and_then(|expression| expression.attrs.get("typeRef"))
                        .cloned(),
                    input_values: dmn_child(child, "inputValues")
                        .and_then(|values| dmn_child(values, "text"))
                        .map(text)
                        .filter(|values| !values.is_empty()),
                });
            }
            "output" => {
                if let Some(output_values) = dmn_child(child, "outputValues") {
                    ensure_feel_expression_language(output_values)?;
                }
                if let Some(default_output) = dmn_child(child, "defaultOutputEntry") {
                    ensure_feel_expression_language(default_output)?;
                }
                let output_values = dmn_child(child, "outputValues")
                    .and_then(|values| dmn_child(values, "text"))
                    .map(text)
                    .map(|values| split_feel_list(&values));
                outputs.push(OutputClause {
                    name: child.attrs.get("name").cloned(),
                    type_ref: child.attrs.get("typeRef").cloned(),
                    default_output_entry: child.attrs.get("defaultOutputEntry").cloned().or_else(
                        || {
                            dmn_child(child, "defaultOutputEntry")
                                .and_then(|entry| dmn_child(entry, "text"))
                                .map(text)
                        },
                    ),
                    output_values,
                });
            }
            "rule" => {
                let input_entries = dmn_children(child, "inputEntry")
                    .map(|entry| {
                        ensure_feel_expression_language(entry)?;
                        Ok(dmn_child(entry, "text")
                            .map(text)
                            .unwrap_or_else(|| text(entry)))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let output_entries = dmn_children(child, "outputEntry")
                    .map(|entry| {
                        ensure_feel_expression_language(entry)?;
                        Ok(dmn_child(entry, "text")
                            .map(text)
                            .unwrap_or_else(|| text(entry)))
                    })
                    .collect::<Result<Vec<_>>>()?;
                rules.push(DecisionRule {
                    id: child.attrs.get("id").cloned(),
                    input_entries,
                    output_entries,
                    annotation_entries: dmn_children(child, "annotationEntry")
                        .map(|entry| {
                            dmn_child(entry, "text")
                                .map(text)
                                .unwrap_or_else(|| text(entry))
                        })
                        .collect(),
                    requirement_ids: requirement_ids(child),
                });
            }
            _ => {}
        }
    }
    if outputs.is_empty() {
        return Err(DmnError::Xml(
            "decisionTable needs at least one output clause".into(),
        ));
    }
    Ok(DecisionTable {
        id: node.attrs.get("id").cloned(),
        hit_policy: HitPolicy::parse(node.attrs.get("hitPolicy").map(String::as_str))?,
        aggregation: node
            .attrs
            .get("aggregation")
            .map(String::as_str)
            .map(Aggregation::parse)
            .transpose()?,
        inputs,
        outputs,
        rules,
    })
}

/// Splits a FEEL comma-separated list without breaking strings, calls, contexts, or ranges.
fn split_feel_list(values: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;

    for (index, character) in values.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = false;
            }
            continue;
        }
        match character {
            '"' => quoted = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                result.push(values[start..index].trim().to_owned());
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    result.push(values[start..].trim().to_owned());
    result
}
