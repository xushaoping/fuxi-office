use super::error::{DmnError, Result};
use super::model::*;
use crate::feel::{FeelExpr, parse_feel};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub(crate) struct CompiledType {
    pub(crate) name: String,
    pub(crate) type_ref: Option<String>,
    pub(crate) is_collection: bool,
    pub(crate) allowed_values: Option<String>,
    pub(crate) components: Vec<CompiledType>,
}

#[derive(Debug, Clone)]
pub struct CompiledDmnModel<'a> {
    pub(crate) model: Cow<'a, DmnModel>,
    pub(crate) decision_ids_by_name: HashMap<String, String>,
    pub(crate) decision_indices_by_id: HashMap<String, usize>,
    pub(crate) input_indices_by_id: HashMap<String, usize>,
    pub(crate) bkm_indices_by_id: HashMap<String, usize>,
    pub(crate) bkm_indices_by_name: HashMap<String, usize>,
    pub(crate) decision_service_indices_by_id: HashMap<String, usize>,
    pub(crate) decision_service_indices_by_name: HashMap<String, usize>,
    pub(crate) decision_order: Vec<String>,
    pub(crate) item_types: HashMap<String, CompiledType>,
    pub(crate) feel_expressions: HashMap<String, FeelExpr>,
    pub(crate) unary_tests: HashMap<String, FeelExpr>,
}

pub fn compile_dmn(model: &DmnModel) -> Result<CompiledDmnModel<'_>> {
    CompiledDmnModel::compile(model)
}

impl<'a> CompiledDmnModel<'a> {
    pub fn compile(model: &'a DmnModel) -> Result<Self> {
        Self::compile_model(Cow::Borrowed(model))
    }

    fn compile_model(model: Cow<'a, DmnModel>) -> Result<Self> {
        let model_ref = model.as_ref();
        validate_model_languages(model_ref)?;
        let item_types = compile_item_types(model_ref)?;
        validate_type_references(model_ref, &item_types)?;
        let mut input_indices_by_id = HashMap::with_capacity(model_ref.input_data.len());
        for (index, input) in model_ref.input_data.iter().enumerate() {
            if input_indices_by_id
                .insert(input.id.clone(), index)
                .is_some()
            {
                return Err(DmnError::DuplicateId(input.id.clone()));
            }
        }
        let mut decision_indices_by_id = HashMap::with_capacity(model_ref.decisions.len());
        for (index, decision) in model_ref.decisions.iter().enumerate() {
            if decision_indices_by_id
                .insert(decision.id.clone(), index)
                .is_some()
            {
                return Err(DmnError::DuplicateId(decision.id.clone()));
            }
        }
        let mut decision_ids_by_name = HashMap::new();
        let mut bkm_indices_by_id =
            HashMap::with_capacity(model_ref.business_knowledge_models.len());
        let mut bkm_indices_by_name =
            HashMap::with_capacity(model_ref.business_knowledge_models.len());
        for (index, bkm) in model_ref.business_knowledge_models.iter().enumerate() {
            if bkm_indices_by_id.insert(bkm.id.clone(), index).is_some() {
                return Err(DmnError::DuplicateId(bkm.id.clone()));
            }
            if bkm_indices_by_name
                .insert(bkm.name.clone(), index)
                .is_some()
            {
                return Err(DmnError::Xml(format!(
                    "duplicate business knowledge model name '{}'",
                    bkm.name
                )));
            }
            if let Some(variable) = &bkm.variable
                && variable != &bkm.name
                && bkm_indices_by_name
                    .insert(variable.clone(), index)
                    .is_some()
            {
                return Err(DmnError::Xml(format!(
                    "duplicate business knowledge model name '{variable}'"
                )));
            }
        }

        for decision in &model_ref.decisions {
            if decision_ids_by_name
                .insert(decision.name.clone(), decision.id.clone())
                .is_some()
            {
                return Err(DmnError::Xml(format!(
                    "duplicate decision name '{}'",
                    decision.name
                )));
            }
            for requirement in &decision.requirements {
                if let Some(input) = &requirement.required_input
                    && !input_indices_by_id.contains_key(input)
                {
                    return Err(DmnError::UnknownReference {
                        decision: decision.name.clone(),
                        reference: input.clone(),
                    });
                }
                if let Some(required) = &requirement.required_decision
                    && !decision_indices_by_id.contains_key(required)
                {
                    return Err(DmnError::UnknownReference {
                        decision: decision.name.clone(),
                        reference: required.clone(),
                    });
                }
            }
            for requirement in &decision.knowledge_requirements {
                if !bkm_indices_by_id.contains_key(&requirement.required_knowledge) {
                    return Err(DmnError::UnknownReference {
                        decision: decision.name.clone(),
                        reference: requirement.required_knowledge.clone(),
                    });
                }
            }
            validate_logic(decision)?;
        }

        for bkm in &model_ref.business_knowledge_models {
            for requirement in &bkm.knowledge_requirements {
                if !bkm_indices_by_id.contains_key(&requirement.required_knowledge) {
                    return Err(DmnError::UnknownReference {
                        decision: bkm.name.clone(),
                        reference: requirement.required_knowledge.clone(),
                    });
                }
            }
            validate_logic_inner(&bkm.logic, &bkm.name)?;
        }
        validate_knowledge_cycles(model_ref, &bkm_indices_by_id)?;
        // Parse each FEEL expression once after structural validation and before graph validation,
        // preserving fail-fast expression diagnostics without a second parser pass.
        let (feel_expressions, unary_tests) = compile_feel_cache(model_ref)?;
        let mut decision_service_indices_by_id =
            HashMap::with_capacity(model_ref.decision_services.len());
        let mut decision_service_indices_by_name =
            HashMap::with_capacity(model_ref.decision_services.len());
        for (index, service) in model_ref.decision_services.iter().enumerate() {
            if decision_service_indices_by_id
                .insert(service.id.clone(), index)
                .is_some()
            {
                return Err(DmnError::DuplicateId(service.id.clone()));
            }
            if decision_service_indices_by_name
                .insert(service.name.clone(), index)
                .is_some()
            {
                return Err(DmnError::InvalidDecisionService {
                    service: service.name.clone(),
                    detail: "duplicate decision service name".into(),
                });
            }
            validate_decision_service(service, &decision_indices_by_id, &input_indices_by_id)?;
        }
        let decision_order = decision_order(model_ref, &decision_indices_by_id)?;

        Ok(Self {
            model,
            decision_ids_by_name,
            decision_indices_by_id,
            input_indices_by_id,
            bkm_indices_by_id,
            bkm_indices_by_name,
            decision_service_indices_by_id,
            decision_service_indices_by_name,
            decision_order,
            item_types,
            feel_expressions,
            unary_tests,
        })
    }

    pub fn model(&self) -> &DmnModel {
        self.model.as_ref()
    }
}

fn validate_model_languages(model: &DmnModel) -> Result<()> {
    if let Some(language) = model
        .expression_language
        .as_deref()
        .filter(|language| !is_feel_language(language))
    {
        return Err(DmnError::UnsupportedFeature {
            decision: model.name.clone().unwrap_or_else(|| "definitions".into()),
            feature: format!("expression language '{language}'"),
        });
    }
    if let Some(language) = model
        .type_language
        .as_deref()
        .filter(|language| !is_feel_language(language))
    {
        return Err(DmnError::UnsupportedFeature {
            decision: model.name.clone().unwrap_or_else(|| "definitions".into()),
            feature: format!("type language '{language}'"),
        });
    }
    Ok(())
}

impl CompiledDmnModel<'static> {
    pub(crate) fn compile_owned(model: DmnModel) -> Result<Self> {
        Self::compile_model(Cow::Owned(model))
    }
}

fn validate_decision_service(
    service: &DecisionService,
    decisions: &HashMap<String, usize>,
    input_data: &HashMap<String, usize>,
) -> Result<()> {
    let mut decision_members = HashMap::new();
    for (role, references) in [
        ("outputDecision", &service.output_decisions),
        ("encapsulatedDecision", &service.encapsulated_decisions),
        ("inputDecision", &service.input_decisions),
    ] {
        for reference in references {
            if !decisions.contains_key(reference) {
                return Err(DmnError::InvalidDecisionService {
                    service: service.name.clone(),
                    detail: format!("{role} references unknown decision '{reference}'"),
                });
            }
            if let Some(previous_role) = decision_members.insert(reference, role) {
                return Err(DmnError::InvalidDecisionService {
                    service: service.name.clone(),
                    detail: format!(
                        "decision '{reference}' is listed as both {previous_role} and {role}"
                    ),
                });
            }
        }
    }
    let mut inputs = HashSet::new();
    for reference in &service.input_data {
        if !input_data.contains_key(reference) {
            return Err(DmnError::InvalidDecisionService {
                service: service.name.clone(),
                detail: format!("inputData references unknown input '{reference}'"),
            });
        }
        if !inputs.insert(reference) {
            return Err(DmnError::InvalidDecisionService {
                service: service.name.clone(),
                detail: format!("duplicate inputData reference '{reference}'"),
            });
        }
    }
    Ok(())
}

fn normalize_builtin_type(type_ref: &str) -> Option<&str> {
    type_ref
        .strip_prefix("feel:")
        .or_else(|| type_ref.strip_prefix("feel."))
        .or(Some(type_ref))
}

fn is_builtin_type(type_ref: &str) -> bool {
    matches!(
        normalize_builtin_type(type_ref),
        Some(
            "Any"
                | "any"
                | "number"
                | "string"
                | "boolean"
                | "date"
                | "time"
                | "date and time"
                | "dateTime"
                | "duration"
                | "yearMonthDuration"
                | "dayTimeDuration"
                | "years and months duration"
                | "days and time duration"
                | "context"
                | "list"
                | "function"
        )
    )
}

fn compile_item_types(model: &DmnModel) -> Result<HashMap<String, CompiledType>> {
    let mut types = HashMap::new();
    for item in &model.item_definitions {
        if types.contains_key(&item.name) {
            return Err(DmnError::InvalidTypeDefinition {
                type_name: item.name.clone(),
                detail: "duplicate item definition".into(),
            });
        }
        types.insert(item.name.clone(), compile_item_type(item)?);
    }
    for name in types.keys() {
        validate_type_cycle(name, &types, &mut Vec::new())?;
    }
    Ok(types)
}

fn compile_item_type(item: &ItemDefinition) -> Result<CompiledType> {
    if let Some(values) = &item.allowed_values {
        parse_unary_tests(values).map_err(|detail| DmnError::InvalidTypeDefinition {
            type_name: item.name.clone(),
            detail: format!("invalid allowedValues expression: {detail}"),
        })?;
    }
    Ok(CompiledType {
        name: item.name.clone(),
        type_ref: item.type_ref.clone(),
        is_collection: item.is_collection,
        allowed_values: item.allowed_values.clone(),
        components: item
            .item_components
            .iter()
            .map(compile_item_type)
            .collect::<Result<Vec<_>>>()?,
    })
}

fn validate_type_cycle(
    name: &str,
    types: &HashMap<String, CompiledType>,
    path: &mut Vec<String>,
) -> Result<()> {
    if let Some(position) = path.iter().position(|item| item == name) {
        return Err(DmnError::InvalidTypeDefinition {
            type_name: name.into(),
            detail: format!("cyclic type reference: {}", path[position..].join(" -> ")),
        });
    }
    let Some(item) = types.get(name) else {
        return Ok(());
    };
    path.push(name.into());
    if let Some(type_ref) = &item.type_ref {
        validate_type_reference(&item.name, type_ref, types)?;
        if types.contains_key(type_ref) {
            validate_type_cycle(type_ref, types, path)?;
        }
    }
    for component in &item.components {
        if let Some(type_ref) = &component.type_ref {
            validate_type_reference(&component.name, type_ref, types)?;
            if types.contains_key(type_ref) {
                validate_type_cycle(type_ref, types, path)?;
            }
        }
    }
    path.pop();
    Ok(())
}

fn validate_type_reference(
    owner: &str,
    type_ref: &str,
    types: &HashMap<String, CompiledType>,
) -> Result<()> {
    if (type_ref.contains(':') && !type_ref.starts_with("feel:"))
        || (!is_builtin_type(type_ref) && !types.contains_key(type_ref))
    {
        return Err(DmnError::UnknownTypeReference {
            owner: owner.into(),
            type_ref: type_ref.into(),
        });
    }
    Ok(())
}

fn validate_type_references(model: &DmnModel, types: &HashMap<String, CompiledType>) -> Result<()> {
    for input in &model.input_data {
        if let Some(type_ref) = &input.type_ref {
            validate_type_reference(&input.name, type_ref, types)?;
        }
    }
    for decision in &model.decisions {
        if let Some(type_ref) = &decision.type_ref {
            validate_type_reference(&decision.name, type_ref, types)?;
        }
        validate_logic_type_references(&decision.logic, &decision.name, types)?;
    }
    for bkm in &model.business_knowledge_models {
        if let Some(type_ref) = &bkm.type_ref {
            validate_type_reference(&bkm.name, type_ref, types)?;
        }
        validate_logic_type_references(&bkm.logic, &bkm.name, types)?;
    }
    Ok(())
}

fn validate_logic_type_references(
    logic: &DecisionLogic,
    owner: &str,
    types: &HashMap<String, CompiledType>,
) -> Result<()> {
    match logic {
        DecisionLogic::DecisionTable(table) => {
            for input in &table.inputs {
                if let Some(type_ref) = &input.type_ref {
                    validate_type_reference(owner, type_ref, types)?;
                }
            }
            for output in &table.outputs {
                if let Some(type_ref) = &output.type_ref {
                    validate_type_reference(owner, type_ref, types)?;
                }
            }
        }
        DecisionLogic::Relation(relation) => {
            for column in &relation.columns {
                if let Some(type_ref) = &column.type_ref {
                    validate_type_reference(owner, type_ref, types)?;
                }
            }
            for row in &relation.rows {
                for entry in &row.entries {
                    validate_logic_type_references(entry, owner, types)?;
                }
            }
        }
        DecisionLogic::Conditional(conditional) => {
            validate_logic_type_references(&conditional.condition, owner, types)?;
            validate_logic_type_references(&conditional.then_expression, owner, types)?;
            validate_logic_type_references(&conditional.else_expression, owner, types)?;
        }
        DecisionLogic::Filter(filter) => {
            validate_logic_type_references(&filter.input, owner, types)?;
            validate_logic_type_references(&filter.matcher, owner, types)?;
        }
        DecisionLogic::For(iterator) => {
            validate_logic_type_references(&iterator.input, owner, types)?;
            validate_logic_type_references(&iterator.return_expression, owner, types)?;
        }
        DecisionLogic::Some(quantified) | DecisionLogic::Every(quantified) => {
            validate_logic_type_references(&quantified.input, owner, types)?;
            validate_logic_type_references(&quantified.satisfies, owner, types)?;
        }
        DecisionLogic::Context(entries) => {
            for entry in entries {
                if let Some(type_ref) = &entry.type_ref {
                    validate_type_reference(owner, type_ref, types)?;
                }
                validate_logic_type_references(&entry.expression, owner, types)?;
            }
        }
        DecisionLogic::List(items) => {
            for item in items {
                validate_logic_type_references(item, owner, types)?;
            }
        }
        DecisionLogic::Invocation(invocation) => {
            validate_logic_type_references(&invocation.expression, owner, types)?;
            for binding in &invocation.bindings {
                validate_logic_type_references(&binding.expression, owner, types)?;
            }
        }
        DecisionLogic::FunctionDefinition(function) => {
            for type_ref in function.parameter_type_refs.iter().flatten() {
                validate_type_reference(owner, type_ref, types)?;
            }
            if let Some(type_ref) = &function.result_type_ref {
                validate_type_reference(owner, type_ref, types)?;
            }
            validate_logic_type_references(&function.body, owner, types)?;
        }
        DecisionLogic::LiteralExpression(_)
        | DecisionLogic::Unsupported(_)
        | DecisionLogic::None => {}
    }
    Ok(())
}

fn decision_order(model: &DmnModel, indices: &HashMap<String, usize>) -> Result<Vec<String>> {
    let mut states = vec![0u8; model.decisions.len()];
    let mut order = Vec::with_capacity(model.decisions.len());
    for index in 0..model.decisions.len() {
        visit_decision(index, model, indices, &mut states, &mut order)?;
    }
    Ok(order)
}

fn validate_knowledge_cycles(model: &DmnModel, indices: &HashMap<String, usize>) -> Result<()> {
    let mut states = vec![0u8; model.business_knowledge_models.len()];
    for index in 0..model.business_knowledge_models.len() {
        visit_knowledge(index, model, indices, &mut states)?;
    }
    Ok(())
}

fn visit_knowledge(
    index: usize,
    model: &DmnModel,
    indices: &HashMap<String, usize>,
    states: &mut [u8],
) -> Result<()> {
    match states[index] {
        2 => return Ok(()),
        1 => {
            return Err(DmnError::CircularDependency(
                model.business_knowledge_models[index].id.clone(),
            ));
        }
        _ => {}
    }
    states[index] = 1;
    for requirement in &model.business_knowledge_models[index].knowledge_requirements {
        let dependency = indices[&requirement.required_knowledge];
        visit_knowledge(dependency, model, indices, states)?;
    }
    states[index] = 2;
    Ok(())
}

fn visit_decision(
    index: usize,
    model: &DmnModel,
    indices: &HashMap<String, usize>,
    states: &mut [u8],
    order: &mut Vec<String>,
) -> Result<()> {
    match states[index] {
        2 => return Ok(()),
        1 => {
            return Err(DmnError::CircularDependency(
                model.decisions[index].id.clone(),
            ));
        }
        _ => {}
    }
    states[index] = 1;
    let decision = &model.decisions[index];
    for requirement in &decision.requirements {
        if let Some(required) = &requirement.required_decision {
            let dependency = *indices
                .get(required)
                .ok_or_else(|| DmnError::UnknownReference {
                    decision: decision.name.clone(),
                    reference: required.clone(),
                })?;
            visit_decision(dependency, model, indices, states, order)?;
        }
    }
    states[index] = 2;
    order.push(decision.id.clone());
    Ok(())
}

fn validate_logic(decision: &Decision) -> Result<()> {
    validate_logic_inner(&decision.logic, &decision.name)
}

fn validate_logic_inner(logic: &DecisionLogic, name: &str) -> Result<()> {
    match logic {
        DecisionLogic::LiteralExpression(_) => Ok(()),
        DecisionLogic::Context(entries) => {
            let mut names = HashSet::new();
            for (index, entry) in entries.iter().enumerate() {
                if entry.name.is_empty() {
                    if index + 1 != entries.len() {
                        return Err(DmnError::Feel {
                            decision: name.into(),
                            detail: "unnamed context result entry must be last".into(),
                        });
                    }
                } else if !names.insert(&entry.name) {
                    return Err(DmnError::Feel {
                        decision: name.into(),
                        detail: format!("duplicate context entry '{}'", entry.name),
                    });
                }
                validate_logic_inner(&entry.expression, name)?;
            }
            Ok(())
        }
        DecisionLogic::List(expressions) => expressions
            .iter()
            .try_for_each(|expression| validate_logic_inner(expression, name)),
        DecisionLogic::Invocation(invocation) => {
            let mut parameters = HashSet::new();
            for binding in &invocation.bindings {
                if !parameters.insert(&binding.parameter) {
                    return Err(DmnError::Feel {
                        decision: name.into(),
                        detail: format!("duplicate invocation binding '{}'", binding.parameter),
                    });
                }
                validate_logic_inner(&binding.expression, name)?;
            }
            validate_logic_inner(&invocation.expression, name)
        }
        DecisionLogic::FunctionDefinition(function) => {
            if let Some(kind) = function
                .kind
                .as_deref()
                .filter(|kind| !kind.eq_ignore_ascii_case("FEEL"))
            {
                return Err(DmnError::UnsupportedFeature {
                    decision: name.into(),
                    feature: format!("{kind} function implementation"),
                });
            }
            if !function.parameter_type_refs.is_empty()
                && function.parameter_type_refs.len() != function.parameters.len()
            {
                return Err(DmnError::Feel {
                    decision: name.into(),
                    detail: "function parameter names and type references are inconsistent".into(),
                });
            }
            let mut parameters = HashSet::new();
            for parameter in &function.parameters {
                if !parameters.insert(parameter) {
                    return Err(DmnError::Feel {
                        decision: name.into(),
                        detail: format!("duplicate function parameter '{parameter}'"),
                    });
                }
            }
            validate_logic_inner(&function.body, name)
        }
        DecisionLogic::Relation(relation) => {
            if relation.columns.is_empty() {
                return Err(DmnError::Feel {
                    decision: name.into(),
                    detail: "relation requires at least one column".into(),
                });
            }
            let mut columns = HashSet::new();
            for column in &relation.columns {
                if column.name.trim().is_empty() {
                    return Err(DmnError::Feel {
                        decision: name.into(),
                        detail: "relation column name must not be empty".into(),
                    });
                }
                if !columns.insert(column.name.as_str()) {
                    return Err(DmnError::Feel {
                        decision: name.into(),
                        detail: format!("duplicate relation column '{}'", column.name),
                    });
                }
            }
            for row in &relation.rows {
                if row.entries.len() != relation.columns.len() {
                    return Err(DmnError::Feel {
                        decision: name.into(),
                        detail: "relation row entry count does not match columns".into(),
                    });
                }
                for entry in &row.entries {
                    validate_logic_inner(entry, name)?;
                }
            }
            Ok(())
        }
        DecisionLogic::Conditional(conditional) => {
            validate_logic_inner(&conditional.condition, name)?;
            validate_logic_inner(&conditional.then_expression, name)?;
            validate_logic_inner(&conditional.else_expression, name)
        }
        DecisionLogic::Filter(filter) => {
            validate_logic_inner(&filter.input, name)?;
            validate_logic_inner(&filter.matcher, name)
        }
        DecisionLogic::For(iterator) => {
            if iterator.variable.trim().is_empty() {
                return Err(DmnError::Feel {
                    decision: name.into(),
                    detail: "for iteratorVariable must not be empty".into(),
                });
            }
            validate_logic_inner(&iterator.input, name)?;
            validate_logic_inner(&iterator.return_expression, name)
        }
        DecisionLogic::Some(quantified) | DecisionLogic::Every(quantified) => {
            if quantified.variable.trim().is_empty() {
                return Err(DmnError::Feel {
                    decision: name.into(),
                    detail: "quantified iteratorVariable must not be empty".into(),
                });
            }
            validate_logic_inner(&quantified.input, name)?;
            validate_logic_inner(&quantified.satisfies, name)
        }
        DecisionLogic::DecisionTable(table) => {
            if table.outputs.is_empty() {
                return Err(DmnError::InvalidDecisionTable {
                    decision: name.into(),
                    detail: "decision table needs an output clause".into(),
                });
            }
            if table.aggregation.is_some() && table.hit_policy != HitPolicy::Collect {
                return Err(DmnError::InvalidDecisionTable {
                    decision: name.into(),
                    detail: "aggregation requires COLLECT hit policy".into(),
                });
            }
            if table.hit_policy == HitPolicy::OutputOrder
                && (table.outputs.len() != 1 || table.outputs[0].output_values.is_none())
            {
                return Err(DmnError::InvalidDecisionTable {
                    decision: name.into(),
                    detail: "OUTPUT ORDER requires one output with outputValues".into(),
                });
            }
            if table.hit_policy == HitPolicy::Priority
                && table
                    .outputs
                    .iter()
                    .all(|output| output.output_values.is_none())
            {
                return Err(DmnError::InvalidDecisionTable {
                    decision: name.into(),
                    detail: "PRIORITY requires outputValues".into(),
                });
            }
            if table.aggregation.is_some() && table.outputs.len() != 1 {
                return Err(DmnError::InvalidDecisionTable {
                    decision: name.into(),
                    detail: "COLLECT aggregation requires a single output".into(),
                });
            }
            if table.outputs.len() > 1 && table.outputs.iter().any(|output| output.name.is_none()) {
                return Err(DmnError::InvalidDecisionTable {
                    decision: name.into(),
                    detail: "multi-output table has unnamed output".into(),
                });
            }
            for rule in &table.rules {
                if rule.input_entries.len() != table.inputs.len()
                    || rule.output_entries.len() != table.outputs.len()
                {
                    return Err(DmnError::InvalidDecisionTable {
                        decision: name.into(),
                        detail: "rule entry count does not match clauses".into(),
                    });
                }
            }
            Ok(())
        }
        DecisionLogic::Unsupported(feature) => Err(DmnError::UnsupportedFeature {
            decision: name.into(),
            feature: feature.clone(),
        }),
        DecisionLogic::None => Ok(()),
    }
}

/// Applies stricter FEEL name binding checks to models created by the DSL/LLM
/// workflow. Runtime DMN execution remains permissive for legacy models that
/// intentionally receive undeclared context values.
pub(crate) fn validate_generated_feel_name_bindings(model: &DmnModel) -> Result<()> {
    let input_indices = model
        .input_data
        .iter()
        .enumerate()
        .map(|(index, input)| (input.id.clone(), index))
        .collect();
    let decision_indices = model
        .decisions
        .iter()
        .enumerate()
        .map(|(index, decision)| (decision.id.clone(), index))
        .collect();
    let bkm_indices = model
        .business_knowledge_models
        .iter()
        .enumerate()
        .map(|(index, bkm)| (bkm.id.clone(), index))
        .collect();
    validate_feel_name_bindings(model, &input_indices, &decision_indices, &bkm_indices)
}

fn validate_feel_name_bindings(
    model: &DmnModel,
    input_indices: &HashMap<String, usize>,
    decision_indices: &HashMap<String, usize>,
    bkm_indices: &HashMap<String, usize>,
) -> Result<()> {
    for decision in &model.decisions {
        // The executor accepts every declared input in its initial context,
        // even when the author omitted an informationRequirement.
        let mut scope = model
            .input_data
            .iter()
            .flat_map(|input| std::iter::once(input.name.clone()).chain(input.variable.clone()))
            .collect::<HashSet<_>>();
        for requirement in &decision.requirements {
            if let Some(input_id) = &requirement.required_input {
                let input = &model.input_data[input_indices[input_id]];
                scope.insert(input.name.clone());
                if let Some(variable) = &input.variable {
                    scope.insert(variable.clone());
                }
            }
            if let Some(decision_id) = &requirement.required_decision {
                let dependency = &model.decisions[decision_indices[decision_id]];
                scope.insert(dependency.name.clone());
                if let Some(variable) = &dependency.variable {
                    scope.insert(variable.clone());
                }
            }
        }
        for requirement in &decision.knowledge_requirements {
            let bkm =
                &model.business_knowledge_models[bkm_indices[&requirement.required_knowledge]];
            scope.insert(bkm.name.clone());
            if let Some(variable) = &bkm.variable {
                scope.insert(variable.clone());
            }
        }
        validate_logic_name_bindings(&decision.logic, &decision.name, &scope, model)?;
    }

    for bkm in &model.business_knowledge_models {
        let mut scope = HashSet::new();
        for requirement in &bkm.knowledge_requirements {
            let dependency =
                &model.business_knowledge_models[bkm_indices[&requirement.required_knowledge]];
            scope.insert(dependency.name.clone());
            if let Some(variable) = &dependency.variable {
                scope.insert(variable.clone());
            }
        }
        validate_logic_name_bindings(&bkm.logic, &bkm.name, &scope, model)?;
    }
    Ok(())
}

fn validate_logic_name_bindings(
    logic: &DecisionLogic,
    owner: &str,
    scope: &HashSet<String>,
    model: &DmnModel,
) -> Result<()> {
    match logic {
        DecisionLogic::LiteralExpression(expression) => {
            validate_expression_name_bindings(expression, owner, scope)?
        }
        DecisionLogic::Context(entries) => {
            let mut context_scope = scope.clone();
            for entry in entries {
                validate_logic_name_bindings(&entry.expression, owner, &context_scope, model)?;
                if !entry.name.is_empty() {
                    context_scope.insert(entry.name.clone());
                }
            }
        }
        DecisionLogic::List(items) => {
            for item in items {
                validate_logic_name_bindings(item, owner, scope, model)?;
            }
        }
        DecisionLogic::Relation(relation) => {
            for row in &relation.rows {
                for entry in &row.entries {
                    validate_logic_name_bindings(entry, owner, scope, model)?;
                }
            }
        }
        DecisionLogic::Conditional(conditional) => {
            validate_logic_name_bindings(&conditional.condition, owner, scope, model)?;
            validate_logic_name_bindings(&conditional.then_expression, owner, scope, model)?;
            validate_logic_name_bindings(&conditional.else_expression, owner, scope, model)?;
        }
        DecisionLogic::Filter(filter) => {
            validate_logic_name_bindings(&filter.input, owner, scope, model)?;
            // A filter predicate receives fields from each runtime list item.
            // Their names cannot be inferred for arbitrary FEEL lists, so keep
            // this dynamic scope validated by execution rather than rejecting
            // valid context-field references during compilation.
        }
        DecisionLogic::For(iterator) => {
            validate_logic_name_bindings(&iterator.input, owner, scope, model)?;
            let mut iterator_scope = scope.clone();
            iterator_scope.insert(iterator.variable.clone());
            // The runtime exposes partial results to iterator expressions when requested.
            iterator_scope.insert("partial".into());
            validate_logic_name_bindings(
                &iterator.return_expression,
                owner,
                &iterator_scope,
                model,
            )?;
        }
        DecisionLogic::Some(quantified) | DecisionLogic::Every(quantified) => {
            validate_logic_name_bindings(&quantified.input, owner, scope, model)?;
            let mut quantified_scope = scope.clone();
            quantified_scope.insert(quantified.variable.clone());
            validate_logic_name_bindings(&quantified.satisfies, owner, &quantified_scope, model)?;
        }
        DecisionLogic::Invocation(invocation) => {
            for binding in &invocation.bindings {
                validate_logic_name_bindings(&binding.expression, owner, scope, model)?;
            }
            if let DecisionLogic::LiteralExpression(target) = invocation.expression.as_ref() {
                let target = target.trim();
                if let Some(bkm) = model
                    .business_knowledge_models
                    .iter()
                    .find(|bkm| bkm.name == target || bkm.variable.as_deref() == Some(target))
                {
                    if let DecisionLogic::FunctionDefinition(function) = &bkm.logic {
                        let formal_parameters = function
                            .parameters
                            .iter()
                            .map(String::as_str)
                            .collect::<HashSet<_>>();
                        for binding in &invocation.bindings {
                            if !formal_parameters.contains(binding.parameter.as_str()) {
                                return Err(DmnError::Feel {
                                    decision: owner.into(),
                                    detail: format!(
                                        "unknown invocation binding '{}' for '{}'; expected one of: {}",
                                        binding.parameter,
                                        bkm.name,
                                        function.parameters.join(", ")
                                    ),
                                });
                            }
                        }
                        for parameter in &function.parameters {
                            if !invocation
                                .bindings
                                .iter()
                                .any(|binding| binding.parameter == *parameter)
                                && !scope.contains(parameter)
                            {
                                return Err(DmnError::Feel {
                                    decision: owner.into(),
                                    detail: format!(
                                        "missing invocation binding '{}' for '{}'",
                                        parameter, bkm.name
                                    ),
                                });
                            }
                        }
                    }
                } else {
                    validate_logic_name_bindings(&invocation.expression, owner, scope, model)?;
                }
            } else {
                validate_logic_name_bindings(&invocation.expression, owner, scope, model)?;
            }
        }
        DecisionLogic::FunctionDefinition(function) => {
            let mut function_scope = scope.clone();
            function_scope.extend(function.parameters.iter().cloned());
            validate_logic_name_bindings(&function.body, owner, &function_scope, model)?;
        }
        DecisionLogic::DecisionTable(table) => {
            for input in &table.inputs {
                validate_expression_name_bindings(&input.expression, owner, scope)?;
                if let Some(values) = &input.input_values {
                    validate_unary_test_name_bindings(values, owner, scope)?;
                }
            }
            for output in &table.outputs {
                if let Some(default) = &output.default_output_entry {
                    validate_expression_name_bindings(default, owner, scope)?;
                }
                if let Some(values) = &output.output_values {
                    for value in values {
                        validate_unary_test_name_bindings(value, owner, scope)?;
                    }
                }
            }
            for rule in &table.rules {
                for test in &rule.input_entries {
                    if !test.trim().is_empty() && test.trim() != "-" {
                        validate_unary_test_name_bindings(test, owner, scope)?;
                    }
                }
                for output in &rule.output_entries {
                    validate_expression_name_bindings(output, owner, scope)?;
                }
            }
        }
        DecisionLogic::Unsupported(_) | DecisionLogic::None => {}
    }
    Ok(())
}

fn validate_unary_test_name_bindings(
    tests: &str,
    owner: &str,
    scope: &HashSet<String>,
) -> Result<()> {
    let _ = scope;
    parse_unary_tests(tests).map_err(|detail| DmnError::Feel {
        decision: owner.into(),
        detail,
    })?;
    Ok(())
}

fn validate_expression_name_bindings(
    expression: &str,
    owner: &str,
    scope: &HashSet<String>,
) -> Result<()> {
    let source = if expression.trim().is_empty() {
        "null"
    } else {
        expression
    };
    let parsed = parse_feel(source).map_err(|detail| DmnError::Feel {
        decision: owner.into(),
        detail,
    })?;
    validate_feel_expression_bindings(&parsed, owner, scope)
}

fn validate_feel_expression_bindings(
    expression: &FeelExpr,
    owner: &str,
    scope: &HashSet<String>,
) -> Result<()> {
    match expression {
        FeelExpr::Name(name) => validate_feel_name(name, owner, scope),
        FeelExpr::QualifiedName(parts) => {
            if let Some(name) = parts.first() {
                validate_feel_name(name, owner, scope)?;
            }
            Ok(())
        }
        FeelExpr::Binary(left, _, right)
        | FeelExpr::Is(left, right)
        | FeelExpr::IsNot(left, right) => {
            validate_feel_expression_bindings(left, owner, scope)?;
            validate_feel_expression_bindings(right, owner, scope)
        }
        FeelExpr::Unary(_, expression)
        | FeelExpr::Not(expression)
        | FeelExpr::Path(expression, _) => {
            validate_feel_expression_bindings(expression, owner, scope)
        }
        FeelExpr::And(expressions) | FeelExpr::Or(expressions) | FeelExpr::List(expressions) => {
            for expression in expressions {
                validate_feel_expression_bindings(expression, owner, scope)?;
            }
            Ok(())
        }
        FeelExpr::Interval { start, end, .. } => {
            validate_feel_expression_bindings(start, owner, scope)?;
            validate_feel_expression_bindings(end, owner, scope)
        }
        FeelExpr::ForExpr { var, iter, body } => {
            validate_feel_expression_bindings(iter, owner, scope)?;
            let mut nested_scope = scope.clone();
            nested_scope.insert(var.clone());
            validate_feel_expression_bindings(body, owner, &nested_scope)
        }
        FeelExpr::Quantified {
            var,
            iter,
            satisfies,
            ..
        } => {
            validate_feel_expression_bindings(iter, owner, scope)?;
            let mut nested_scope = scope.clone();
            nested_scope.insert(var.clone());
            validate_feel_expression_bindings(satisfies, owner, &nested_scope)
        }
        FeelExpr::IfExpr { cond, then_, else_ } | FeelExpr::IfThenElse { cond, then_, else_ } => {
            validate_feel_expression_bindings(cond, owner, scope)?;
            validate_feel_expression_bindings(then_, owner, scope)?;
            validate_feel_expression_bindings(else_, owner, scope)
        }
        FeelExpr::Call(_, arguments) => {
            for argument in arguments {
                validate_feel_expression_bindings(argument, owner, scope)?;
            }
            Ok(())
        }
        FeelExpr::Context(entries) => {
            let mut context_scope = scope.clone();
            for (name, expression) in entries {
                validate_feel_expression_bindings(expression, owner, &context_scope)?;
                context_scope.insert(name.clone());
            }
            Ok(())
        }
        FeelExpr::Filter(input, matcher) => {
            validate_feel_expression_bindings(input, owner, scope)?;
            // FEEL filter predicates can refer directly to dynamic context
            // fields supplied by list items, which static compilation cannot
            // determine from an arbitrary list expression.
            let _ = matcher;
            Ok(())
        }
        FeelExpr::Index(input, index) => {
            validate_feel_expression_bindings(input, owner, scope)?;
            validate_feel_expression_bindings(index, owner, scope)
        }
        FeelExpr::FunctionDef(parameters, body) => {
            let mut function_scope = scope.clone();
            function_scope.extend(parameters.iter().cloned());
            validate_feel_expression_bindings(body, owner, &function_scope)
        }
        FeelExpr::Between { value, low, high } => {
            validate_feel_expression_bindings(value, owner, scope)?;
            validate_feel_expression_bindings(low, owner, scope)?;
            validate_feel_expression_bindings(high, owner, scope)
        }
        FeelExpr::InstanceOf { value, .. } => {
            validate_feel_expression_bindings(value, owner, scope)
        }
        FeelExpr::InUnaryTests { value, tests } => {
            validate_feel_expression_bindings(value, owner, scope)?;
            for test in tests {
                match test {
                    crate::feel::UnaryTest::LessThan(value)
                    | crate::feel::UnaryTest::LessOrEqual(value)
                    | crate::feel::UnaryTest::GreaterThan(value)
                    | crate::feel::UnaryTest::GreaterOrEqual(value)
                    | crate::feel::UnaryTest::Range(value)
                    | crate::feel::UnaryTest::Value(value) => {
                        validate_feel_expression_bindings(value, owner, scope)?;
                    }
                }
            }
            Ok(())
        }
        FeelExpr::CallNamed { args, .. } => {
            for (_, argument) in args {
                validate_feel_expression_bindings(argument, owner, scope)?;
            }
            Ok(())
        }
        FeelExpr::Null
        | FeelExpr::Bool(_)
        | FeelExpr::Number(_)
        | FeelExpr::String(_)
        | FeelExpr::DateLit(_)
        | FeelExpr::DateTimeLit(_)
        | FeelExpr::AtLiteral(_) => Ok(()),
    }
}

fn validate_feel_name(name: &str, owner: &str, scope: &HashSet<String>) -> Result<()> {
    if scope.contains(name) {
        return Ok(());
    }
    Err(DmnError::Feel {
        decision: owner.into(),
        detail: format!(
            "undefined FEEL name '{name}'; use a quoted string literal when a fixed text value is intended"
        ),
    })
}

fn compile_feel_cache(
    model: &DmnModel,
) -> Result<(HashMap<String, FeelExpr>, HashMap<String, FeelExpr>)> {
    let mut expressions = HashMap::new();
    let mut unary_tests = HashMap::new();
    for item in &model.item_definitions {
        cache_item_unary_tests(item, &mut unary_tests)?;
    }
    for decision in &model.decisions {
        cache_logic(
            &decision.logic,
            &decision.name,
            &mut expressions,
            &mut unary_tests,
        )?;
    }
    for bkm in &model.business_knowledge_models {
        cache_logic(&bkm.logic, &bkm.name, &mut expressions, &mut unary_tests)?;
    }
    Ok((expressions, unary_tests))
}

fn cache_expression(
    expression: &str,
    owner: &str,
    expressions: &mut HashMap<String, FeelExpr>,
) -> Result<()> {
    if !expressions.contains_key(expression) {
        let source = if expression.trim().is_empty() {
            "null"
        } else {
            expression
        };
        let parsed = parse_feel(source).map_err(|detail| DmnError::Feel {
            decision: owner.into(),
            detail,
        })?;
        expressions.insert(expression.into(), parsed);
    }
    Ok(())
}

fn cache_unary_tests(
    tests: &str,
    owner: &str,
    unary_tests: &mut HashMap<String, FeelExpr>,
) -> Result<()> {
    if !unary_tests.contains_key(tests) {
        let parsed = parse_unary_tests(tests).map_err(|detail| DmnError::Feel {
            decision: owner.into(),
            detail,
        })?;
        unary_tests.insert(tests.into(), parsed);
    }
    Ok(())
}

fn parse_unary_tests(tests: &str) -> std::result::Result<FeelExpr, String> {
    let tests = tests.trim();
    if let Some(equal) = tests.strip_prefix('=')
        && !equal.starts_with(['=', '<', '>', '!'])
    {
        return parse_feel(&format!("__input = ({})", equal.trim_start()));
    }
    if let Some(negated) = tests.strip_prefix("not") {
        let negated = negated.trim_start();
        if let Some(inner) = negated
            .strip_prefix('(')
            .and_then(|inner| inner.strip_suffix(')'))
        {
            let inner = inner.trim();
            // Equality is implicit in a unary test. Keeping the optional
            // leading '=' inside the parenthesized list produces an invalid
            // FEEL expression for forms such as not(= "A").
            let inner = inner.strip_prefix('=').map(str::trim).unwrap_or(inner);
            return parse_feel(&format!("not (__input in ({inner}))"));
        }
    }
    // A decision-table inputEntry is already a unary-test context. Some DSLs
    // use the readable form `in ("A", "B")`; avoid wrapping that form in a
    // second `in (...)`, which would otherwise make FEEL parse `in` as a
    // primary expression.
    if let Some(rest) = tests
        .strip_prefix("in")
        .or_else(|| tests.strip_prefix("In"))
    {
        let rest = rest.trim_start();
        if rest.starts_with('(') {
            return parse_feel(&format!("__input in {rest}"));
        }
    }
    parse_feel(&format!("__input in ({tests})"))
}

fn cache_item_unary_tests(
    item: &ItemDefinition,
    unary_tests: &mut HashMap<String, FeelExpr>,
) -> Result<()> {
    if let Some(values) = &item.allowed_values {
        cache_unary_tests(values, &item.name, unary_tests)?;
    }
    for component in &item.item_components {
        cache_item_unary_tests(component, unary_tests)?;
    }
    Ok(())
}

fn cache_logic(
    logic: &DecisionLogic,
    owner: &str,
    expressions: &mut HashMap<String, FeelExpr>,
    unary_tests: &mut HashMap<String, FeelExpr>,
) -> Result<()> {
    match logic {
        DecisionLogic::LiteralExpression(expression) => {
            cache_expression(expression, owner, expressions)?
        }
        DecisionLogic::Context(entries) => {
            for entry in entries {
                cache_logic(&entry.expression, owner, expressions, unary_tests)?;
            }
        }
        DecisionLogic::List(items) => {
            for item in items {
                cache_logic(item, owner, expressions, unary_tests)?;
            }
        }
        DecisionLogic::Relation(relation) => {
            for row in &relation.rows {
                for entry in &row.entries {
                    cache_logic(entry, owner, expressions, unary_tests)?;
                }
            }
        }
        DecisionLogic::Conditional(conditional) => {
            cache_logic(&conditional.condition, owner, expressions, unary_tests)?;
            cache_logic(
                &conditional.then_expression,
                owner,
                expressions,
                unary_tests,
            )?;
            cache_logic(
                &conditional.else_expression,
                owner,
                expressions,
                unary_tests,
            )?;
        }
        DecisionLogic::Filter(filter) => {
            cache_logic(&filter.input, owner, expressions, unary_tests)?;
            cache_logic(&filter.matcher, owner, expressions, unary_tests)?;
        }
        DecisionLogic::For(iterator) => {
            cache_logic(&iterator.input, owner, expressions, unary_tests)?;
            cache_logic(&iterator.return_expression, owner, expressions, unary_tests)?;
        }
        DecisionLogic::Some(quantified) | DecisionLogic::Every(quantified) => {
            cache_logic(&quantified.input, owner, expressions, unary_tests)?;
            cache_logic(&quantified.satisfies, owner, expressions, unary_tests)?;
        }
        DecisionLogic::Invocation(invocation) => {
            cache_logic(&invocation.expression, owner, expressions, unary_tests)?;
            for binding in &invocation.bindings {
                cache_logic(&binding.expression, owner, expressions, unary_tests)?;
            }
        }
        DecisionLogic::FunctionDefinition(function) => {
            cache_logic(&function.body, owner, expressions, unary_tests)?
        }
        DecisionLogic::DecisionTable(table) => {
            for input in &table.inputs {
                cache_expression(&input.expression, owner, expressions)?;
                if let Some(values) = &input.input_values {
                    cache_unary_tests(values, owner, unary_tests)?;
                }
            }
            for output in &table.outputs {
                if let Some(default) = &output.default_output_entry {
                    cache_expression(default, owner, expressions)?;
                }
                if let Some(values) = &output.output_values {
                    for value in values {
                        cache_unary_tests(value, owner, unary_tests)?;
                    }
                }
            }
            for rule in &table.rules {
                for test in &rule.input_entries {
                    let test = test.trim();
                    if !test.is_empty() && test != "-" {
                        cache_unary_tests(test, owner, unary_tests)?;
                    }
                }
                for output in &rule.output_entries {
                    cache_expression(output, owner, expressions)?;
                }
            }
        }
        DecisionLogic::Unsupported(_) | DecisionLogic::None => {}
    }
    Ok(())
}
