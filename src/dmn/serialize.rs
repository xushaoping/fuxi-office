use super::error::{DmnError, Result};
use super::model::*;

const FUXI_TRACE_NS: &str = "urn:fuxi:dmn:traceability:v1";

impl DmnModel {
    pub fn to_dmn_xml(&self) -> Result<String> {
        let mut output = format!("<definitions xmlns=\"{}\"", DMN_NAMESPACE);
        output.push_str(&format!(" xmlns:fuxi=\"{}\"", FUXI_TRACE_NS));
        attribute(&mut output, "name", self.name.as_deref());
        attribute(&mut output, "namespace", self.namespace.as_deref());
        attribute(
            &mut output,
            "expressionLanguage",
            self.expression_language.as_deref(),
        );
        attribute(&mut output, "typeLanguage", self.type_language.as_deref());
        output.push('>');
        for import in &self.imports {
            output.push_str("<import");
            attribute(&mut output, "id", import.id.as_deref());
            attribute(&mut output, "name", import.name.as_deref());
            attribute(&mut output, "namespace", import.namespace.as_deref());
            attribute(&mut output, "locationURI", import.location_uri.as_deref());
            output.push_str("/>");
        }
        for item in &self.item_definitions {
            write_item_definition(&mut output, item, "itemDefinition");
        }
        for input in &self.input_data {
            output.push_str("<inputData");
            attribute(&mut output, "id", Some(&input.id));
            attribute(&mut output, "name", Some(&input.name));
            output.push('>');
            if input.variable.is_some() || input.type_ref.is_some() {
                output.push_str("<variable");
                attribute(
                    &mut output,
                    "name",
                    Some(input.variable.as_deref().unwrap_or(&input.name)),
                );
                attribute(&mut output, "typeRef", input.type_ref.as_deref());
                output.push_str("/>");
            }
            output.push_str("</inputData>");
        }
        for bkm in &self.business_knowledge_models {
            output.push_str("<businessKnowledgeModel");
            attribute(&mut output, "id", Some(&bkm.id));
            attribute(&mut output, "name", Some(&bkm.name));
            output.push('>');
            if bkm.variable.is_some() || bkm.type_ref.is_some() {
                output.push_str("<variable");
                attribute(
                    &mut output,
                    "name",
                    Some(bkm.variable.as_deref().unwrap_or(&bkm.name)),
                );
                attribute(&mut output, "typeRef", bkm.type_ref.as_deref());
                output.push_str("/>");
            }
            write_knowledge_requirements(&mut output, &bkm.knowledge_requirements);
            write_bkm_logic(&mut output, &bkm.logic)?;
            output.push_str("</businessKnowledgeModel>");
        }
        for decision in &self.decisions {
            output.push_str("<decision");
            attribute(&mut output, "id", Some(&decision.id));
            attribute(&mut output, "name", Some(&decision.name));
            output.push('>');
            write_requirement_trace(&mut output, &decision.requirement_ids);
            if decision.variable.is_some() || decision.type_ref.is_some() {
                output.push_str("<variable");
                attribute(
                    &mut output,
                    "name",
                    Some(decision.variable.as_deref().unwrap_or(&decision.name)),
                );
                attribute(&mut output, "typeRef", decision.type_ref.as_deref());
                output.push_str("/>");
            }
            for requirement in &decision.requirements {
                output.push_str("<informationRequirement");
                attribute(&mut output, "id", requirement.id.as_deref());
                output.push('>');
                if let Some(input) = &requirement.required_input {
                    output.push_str("<requiredInput");
                    attribute(&mut output, "href", Some(&format!("#{input}")));
                    output.push_str("/>");
                }
                if let Some(required) = &requirement.required_decision {
                    output.push_str("<requiredDecision");
                    attribute(&mut output, "href", Some(&format!("#{required}")));
                    output.push_str("/>");
                }
                output.push_str("</informationRequirement>");
            }
            write_knowledge_requirements(&mut output, &decision.knowledge_requirements);
            write_logic(&mut output, &decision.logic)?;
            output.push_str("</decision>");
        }
        for service in &self.decision_services {
            output.push_str("<decisionService");
            attribute(&mut output, "id", Some(&service.id));
            attribute(&mut output, "name", Some(&service.name));
            if service.output_decisions.is_empty()
                && service.encapsulated_decisions.is_empty()
                && service.input_decisions.is_empty()
                && service.input_data.is_empty()
            {
                output.push_str("/>");
                continue;
            }
            output.push('>');
            for decision in &service.output_decisions {
                reference_element(&mut output, "outputDecision", decision);
            }
            for decision in &service.encapsulated_decisions {
                reference_element(&mut output, "encapsulatedDecision", decision);
            }
            for decision in &service.input_decisions {
                reference_element(&mut output, "inputDecision", decision);
            }
            for input in &service.input_data {
                reference_element(&mut output, "inputData", input);
            }
            output.push_str("</decisionService>");
        }
        output.push_str("</definitions>");
        Ok(output)
    }
}

/// Serializes a boxed expression for editors that preserve unsupported expression kinds.
#[allow(dead_code)] // 编辑器集成预留 API：当前模块随 dmn 测试构建编译，暂无调用者
pub fn decision_logic_to_dmn_xml(logic: &DecisionLogic) -> Result<String> {
    let mut output = String::new();
    write_logic(&mut output, logic)?;
    Ok(output)
}

/// Serializes an encapsulated BKM body, including its formal parameters when present.
#[allow(dead_code)] // 编辑器集成预留 API（BKM 对称序列化），暂无调用者
pub fn bkm_logic_to_dmn_xml(logic: &DecisionLogic) -> Result<String> {
    let mut output = String::new();
    write_bkm_logic(&mut output, logic)?;
    Ok(output)
}

fn write_knowledge_requirements(output: &mut String, requirements: &[KnowledgeRequirement]) {
    for requirement in requirements {
        output.push_str("<knowledgeRequirement");
        attribute(output, "id", requirement.id.as_deref());
        output.push('>');
        reference_element(output, "requiredKnowledge", &requirement.required_knowledge);
        output.push_str("</knowledgeRequirement>");
    }
}

fn write_requirement_trace(output: &mut String, requirement_ids: &[String]) {
    if requirement_ids.is_empty() {
        return;
    }
    output.push_str("<extensionElements><fuxi:traceability>");
    for id in requirement_ids {
        output.push_str("<fuxi:requirementRef");
        attribute(output, "id", Some(id));
        output.push_str("/>");
    }
    output.push_str("</fuxi:traceability></extensionElements>");
}

fn write_bkm_logic(output: &mut String, logic: &DecisionLogic) -> Result<()> {
    output.push_str("<encapsulatedLogic");
    match logic {
        DecisionLogic::FunctionDefinition(function) => {
            attribute(output, "kind", function.kind.as_deref());
            attribute(output, "typeRef", function.result_type_ref.as_deref());
            output.push('>');
            write_formal_parameters(output, function);
            write_logic(output, &function.body)?;
        }
        other => {
            output.push('>');
            write_logic(output, other)?;
        }
    }
    output.push_str("</encapsulatedLogic>");
    Ok(())
}

fn reference_element(output: &mut String, element: &str, reference: &str) {
    output.push('<');
    output.push_str(element);
    attribute(output, "href", Some(&format!("#{reference}")));
    output.push_str("/>");
}

fn write_item_definition(output: &mut String, item: &ItemDefinition, element: &str) {
    output.push('<');
    output.push_str(element);
    attribute(output, "id", item.id.as_deref());
    attribute(output, "name", Some(&item.name));
    if item.is_collection {
        attribute(output, "isCollection", Some("true"));
    }
    output.push('>');
    if let Some(type_ref) = &item.type_ref {
        output.push_str("<typeRef>");
        output.push_str(&escape(type_ref));
        output.push_str("</typeRef>");
    }
    if let Some(values) = &item.allowed_values {
        output.push_str("<allowedValues>");
        text(output, values);
        output.push_str("</allowedValues>");
    }
    for component in &item.item_components {
        write_item_definition(output, component, "itemComponent");
    }
    output.push_str("</");
    output.push_str(element);
    output.push('>');
}

fn write_logic(output: &mut String, logic: &DecisionLogic) -> Result<()> {
    match logic {
        DecisionLogic::LiteralExpression(expression) => {
            text_element(output, "literalExpression", expression)
        }
        DecisionLogic::Context(entries) => {
            output.push_str("<context>");
            for entry in entries {
                output.push_str("<contextEntry>");
                if !entry.name.is_empty() {
                    output.push_str("<variable");
                    attribute(output, "name", Some(&entry.name));
                    attribute(output, "typeRef", entry.type_ref.as_deref());
                    output.push_str("/>");
                }
                write_logic(output, &entry.expression)?;
                output.push_str("</contextEntry>");
            }
            output.push_str("</context>");
        }
        DecisionLogic::List(expressions) => {
            output.push_str("<list>");
            for expression in expressions {
                output.push_str("<expression>");
                write_logic(output, expression)?;
                output.push_str("</expression>");
            }
            output.push_str("</list>");
        }
        DecisionLogic::Relation(relation) => write_relation(output, relation)?,
        DecisionLogic::Conditional(conditional) => {
            output.push_str("<conditional>");
            write_wrapped_logic(output, "if", &conditional.condition)?;
            write_wrapped_logic(output, "then", &conditional.then_expression)?;
            write_wrapped_logic(output, "else", &conditional.else_expression)?;
            output.push_str("</conditional>");
        }
        DecisionLogic::Filter(filter) => {
            output.push_str("<filter>");
            write_wrapped_logic(output, "in", &filter.input)?;
            write_wrapped_logic(output, "match", &filter.matcher)?;
            output.push_str("</filter>");
        }
        DecisionLogic::For(iterator) => {
            output.push_str("<for");
            attribute(output, "iteratorVariable", Some(&iterator.variable));
            output.push('>');
            write_wrapped_logic(output, "in", &iterator.input)?;
            write_wrapped_logic(output, "return", &iterator.return_expression)?;
            output.push_str("</for>");
        }
        DecisionLogic::Some(quantified) | DecisionLogic::Every(quantified) => {
            let element = if matches!(logic, DecisionLogic::Some(_)) {
                "some"
            } else {
                "every"
            };
            output.push('<');
            output.push_str(element);
            attribute(output, "iteratorVariable", Some(&quantified.variable));
            output.push('>');
            write_wrapped_logic(output, "in", &quantified.input)?;
            write_wrapped_logic(output, "satisfies", &quantified.satisfies)?;
            output.push_str("</");
            output.push_str(element);
            output.push('>');
        }
        DecisionLogic::Invocation(invocation) => {
            output.push_str("<invocation><expression>");
            write_logic(output, &invocation.expression)?;
            output.push_str("</expression>");
            for binding in &invocation.bindings {
                output.push_str("<binding><parameter");
                attribute(output, "name", Some(&binding.parameter));
                output.push_str("/><expression>");
                write_logic(output, &binding.expression)?;
                output.push_str("</expression></binding>");
            }
            output.push_str("</invocation>");
        }
        DecisionLogic::FunctionDefinition(function) => {
            output.push_str("<functionDefinition");
            attribute(output, "kind", function.kind.as_deref());
            attribute(output, "typeRef", function.result_type_ref.as_deref());
            output.push('>');
            write_formal_parameters(output, function);
            output.push_str("<expression>");
            write_logic(output, &function.body)?;
            output.push_str("</expression></functionDefinition>");
        }
        DecisionLogic::DecisionTable(table) => write_table(output, table),
        DecisionLogic::Unsupported(feature) => {
            return Err(DmnError::UnsupportedFeature {
                decision: "serialization".into(),
                feature: feature.clone(),
            });
        }
        DecisionLogic::None => {
            return Err(DmnError::UnsupportedFeature {
                decision: "serialization".into(),
                feature: "no decision logic".into(),
            });
        }
    }
    Ok(())
}

fn write_formal_parameters(output: &mut String, function: &FunctionDefinition) {
    for (index, parameter) in function.parameters.iter().enumerate() {
        output.push_str("<formalParameter");
        attribute(output, "name", Some(parameter));
        attribute(
            output,
            "typeRef",
            function
                .parameter_type_refs
                .get(index)
                .and_then(Option::as_deref),
        );
        output.push_str("/>");
    }
}

fn write_wrapped_logic(output: &mut String, element: &str, logic: &DecisionLogic) -> Result<()> {
    output.push('<');
    output.push_str(element);
    output.push('>');
    write_logic(output, logic)?;
    output.push_str("</");
    output.push_str(element);
    output.push('>');
    Ok(())
}

fn write_relation(output: &mut String, relation: &Relation) -> Result<()> {
    output.push_str("<relation");
    attribute(output, "id", relation.id.as_deref());
    output.push('>');
    for column in &relation.columns {
        output.push_str("<column");
        attribute(output, "id", column.id.as_deref());
        attribute(output, "name", Some(&column.name));
        attribute(output, "typeRef", column.type_ref.as_deref());
        output.push_str("/>");
    }
    for row in &relation.rows {
        output.push_str("<row");
        attribute(output, "id", row.id.as_deref());
        output.push('>');
        for entry in &row.entries {
            write_logic(output, entry)?;
        }
        output.push_str("</row>");
    }
    output.push_str("</relation>");
    Ok(())
}

fn write_table(output: &mut String, table: &DecisionTable) {
    output.push_str("<decisionTable");
    attribute(output, "id", table.id.as_deref());
    attribute(output, "hitPolicy", Some(hit_policy_name(table.hit_policy)));
    if let Some(aggregation) = table.aggregation {
        attribute(output, "aggregation", Some(aggregation_name(aggregation)));
    }
    output.push('>');
    for input in &table.inputs {
        output.push_str("<input><inputExpression");
        attribute(output, "name", input.variable.as_deref());
        attribute(output, "typeRef", input.type_ref.as_deref());
        output.push('>');
        text(output, &input.expression);
        output.push_str("</inputExpression>");
        if let Some(values) = &input.input_values {
            text_element(output, "inputValues", values);
        }
        output.push_str("</input>");
    }
    for clause in &table.outputs {
        output.push_str("<output");
        attribute(output, "name", clause.name.as_deref());
        attribute(output, "typeRef", clause.type_ref.as_deref());
        attribute(
            output,
            "defaultOutputEntry",
            clause.default_output_entry.as_deref(),
        );
        output.push('>');
        if let Some(values) = &clause.output_values {
            output.push_str("<outputValues>");
            text(output, &values.join(", "));
            output.push_str("</outputValues>");
        }
        output.push_str("</output>");
    }
    for rule in &table.rules {
        output.push_str("<rule");
        attribute(output, "id", rule.id.as_deref());
        output.push('>');
        write_requirement_trace(output, &rule.requirement_ids);
        for entry in &rule.input_entries {
            text_element(output, "inputEntry", entry);
        }
        for entry in &rule.output_entries {
            text_element(output, "outputEntry", entry);
        }
        for entry in &rule.annotation_entries {
            text_element(output, "annotationEntry", entry);
        }
        output.push_str("</rule>");
    }
    output.push_str("</decisionTable>");
}

fn text_element(output: &mut String, element: &str, value: &str) {
    output.push('<');
    output.push_str(element);
    output.push('>');
    text(output, value);
    output.push_str("</");
    output.push_str(element);
    output.push('>');
}
fn text(output: &mut String, value: &str) {
    output.push_str("<text>");
    output.push_str(&escape(value));
    output.push_str("</text>");
}
fn attribute(output: &mut String, name: &str, value: Option<&str>) {
    if let Some(value) = value {
        output.push(' ');
        output.push_str(name);
        output.push_str("=\"");
        output.push_str(&escape(value));
        output.push('"');
    }
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn hit_policy_name(value: HitPolicy) -> &'static str {
    match value {
        HitPolicy::Unique => "UNIQUE",
        HitPolicy::First => "FIRST",
        HitPolicy::Any => "ANY",
        HitPolicy::Priority => "PRIORITY",
        HitPolicy::RuleOrder => "RULE ORDER",
        HitPolicy::OutputOrder => "OUTPUT ORDER",
        HitPolicy::Collect => "COLLECT",
    }
}
fn aggregation_name(value: Aggregation) -> &'static str {
    match value {
        Aggregation::Sum => "SUM",
        Aggregation::Min => "MIN",
        Aggregation::Max => "MAX",
        Aggregation::Count => "COUNT",
    }
}
