use super::compile::{CompiledDmnModel, CompiledType, compile_dmn};
use super::diagnostics::*;
use super::error::{DmnError, Result};
use super::model::*;
use super::result::*;
use crate::feel::{FeelInterpreter, FeelValue};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

fn logic_mentions_name(logic: &DecisionLogic, name: &str) -> bool {
    match logic {
        DecisionLogic::LiteralExpression(expression) => expression
            .split(|character: char| !(character.is_alphanumeric() || character == '_'))
            .any(|token| token == name),
        DecisionLogic::Context(entries) => entries
            .iter()
            .any(|entry| logic_mentions_name(&entry.expression, name)),
        DecisionLogic::List(items) => items.iter().any(|item| logic_mentions_name(item, name)),
        DecisionLogic::Relation(relation) => relation.rows.iter().any(|row| {
            row.entries
                .iter()
                .any(|entry| logic_mentions_name(entry, name))
        }),
        DecisionLogic::Conditional(conditional) => {
            logic_mentions_name(&conditional.condition, name)
                || logic_mentions_name(&conditional.then_expression, name)
                || logic_mentions_name(&conditional.else_expression, name)
        }
        DecisionLogic::Filter(filter) => {
            logic_mentions_name(&filter.input, name) || logic_mentions_name(&filter.matcher, name)
        }
        DecisionLogic::For(iterator) => {
            logic_mentions_name(&iterator.input, name)
                || (iterator.variable != name
                    && logic_mentions_name(&iterator.return_expression, name))
        }
        DecisionLogic::Some(quantified) | DecisionLogic::Every(quantified) => {
            logic_mentions_name(&quantified.input, name)
                || (quantified.variable != name && logic_mentions_name(&quantified.satisfies, name))
        }
        DecisionLogic::Invocation(invocation) => {
            logic_mentions_name(&invocation.expression, name)
                || invocation
                    .bindings
                    .iter()
                    .any(|binding| logic_mentions_name(&binding.expression, name))
        }
        DecisionLogic::FunctionDefinition(function) => {
            !function
                .parameters
                .iter()
                .any(|parameter| parameter == name)
                && logic_mentions_name(&function.body, name)
        }
        DecisionLogic::DecisionTable(_) | DecisionLogic::Unsupported(_) | DecisionLogic::None => {
            false
        }
    }
}

fn bkm_has_literal_feel_body(bkm: &BusinessKnowledgeModel) -> bool {
    matches!(
        &bkm.logic,
        DecisionLogic::FunctionDefinition(function)
            if matches!(function.body.as_ref(), DecisionLogic::LiteralExpression(_))
    )
}

// Older generated models could wrap a BKM body in a second functionDefinition
// even though formal parameters are already declared on encapsulatedLogic.
// When the nested parameters are all bound by the outer invocation, unwrap the
// redundant function value and evaluate the actual body. New DSL validation
// prevents this shape, but published models must remain executable.
fn unwrap_redundant_function_body<'a>(
    mut logic: &'a DecisionLogic,
    context: &HashMap<String, FeelValue>,
) -> &'a DecisionLogic {
    loop {
        let DecisionLogic::FunctionDefinition(function) = logic else {
            return logic;
        };
        if function
            .parameters
            .iter()
            .all(|parameter| context.contains_key(parameter))
        {
            logic = function.body.as_ref();
        } else {
            return logic;
        }
    }
}

pub struct DmnExecutor<'a> {
    source: ExecutorSource<'a>,
}

enum ExecutorSource<'a> {
    Model(Box<Result<CompiledDmnModel<'a>>>),
    // 仅从 from_compiled（按名/ID 单决策执行等预留 API）构造
    #[cfg_attr(not(test), allow(dead_code))]
    Compiled(&'a CompiledDmnModel<'a>),
}

fn type_violation(location: &str, type_ref: &str, detail: &str) -> DmnError {
    DmnError::TypeConstraintViolation {
        location: location.into(),
        type_ref: type_ref.into(),
        detail: detail.into(),
    }
}

impl<'a> DmnExecutor<'a> {
    pub fn new(model: &'a DmnModel) -> Self {
        Self {
            source: ExecutorSource::Model(Box::new(compile_dmn(model))),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))] // 复用已编译模型：当前仅单决策执行/决策服务预留 API 与测试使用
    pub fn from_compiled(model: &'a CompiledDmnModel<'a>) -> Self {
        Self {
            source: ExecutorSource::Compiled(model),
        }
    }

    fn compiled(&self) -> Result<&CompiledDmnModel<'a>> {
        match &self.source {
            ExecutorSource::Model(compiled) => compiled.as_ref().as_ref().map_err(Clone::clone),
            ExecutorSource::Compiled(compiled) => Ok(compiled),
        }
    }

    fn eval_expression(
        &self,
        compiled: &CompiledDmnModel<'_>,
        expression: &str,
        context: &HashMap<String, FeelValue>,
        decision: &str,
    ) -> Result<FeelValue> {
        let ast = compiled
            .feel_expressions
            .get(expression)
            .ok_or_else(|| DmnError::Feel {
                decision: decision.into(),
                detail: "expression was not compiled".into(),
            })?;
        FeelInterpreter::new()
            .eval_expr(ast, context)
            .map_err(|detail| DmnError::Feel {
                decision: decision.into(),
                detail,
            })
    }

    fn eval_unary_tests(
        &self,
        compiled: &CompiledDmnModel<'_>,
        tests: &str,
        value: &FeelValue,
        context: &mut HashMap<String, FeelValue>,
        decision: &str,
    ) -> Result<bool> {
        let ast = compiled
            .unary_tests
            .get(tests)
            .ok_or_else(|| DmnError::Feel {
                decision: decision.into(),
                detail: "unary tests were not compiled".into(),
            })?;
        let previous = context.insert("__input".into(), value.clone());
        let result = FeelInterpreter::new().eval_expr(ast, context);
        match previous {
            Some(previous) => {
                context.insert("__input".into(), previous);
            }
            None => {
                context.remove("__input");
            }
        }
        result
            .map(|result| result == FeelValue::Bool(true))
            .map_err(|detail| DmnError::Feel {
                decision: decision.into(),
                detail,
            })
    }

    fn with_context_bindings<T>(
        state: &mut ExecutionState,
        bindings: Vec<(String, FeelValue)>,
        evaluate: impl FnOnce(&mut ExecutionState) -> Result<T>,
    ) -> Result<T> {
        let mut previous = Vec::with_capacity(bindings.len());
        for (name, value) in bindings {
            let old_value = state.context.insert(name.clone(), value);
            previous.push((name, old_value));
        }
        let result = evaluate(state);
        for (name, old_value) in previous.into_iter().rev() {
            if let Some(old_value) = old_value {
                state.context.insert(name, old_value);
            } else {
                state.context.remove(&name);
            }
        }
        result
    }

    fn restore_context_bindings(
        state: &mut ExecutionState,
        bindings: Vec<(String, Option<FeelValue>)>,
    ) {
        for (name, old_value) in bindings.into_iter().rev() {
            if let Some(old_value) = old_value {
                state.context.insert(name, old_value);
            } else {
                state.context.remove(&name);
            }
        }
    }

    fn expand_iterable(value: FeelValue, decision: &str) -> Result<Vec<FeelValue>> {
        match value {
            FeelValue::List(items) => Ok(items),
            FeelValue::Range {
                start,
                end,
                start_open,
                end_open,
            } => {
                let (FeelValue::Number(start), FeelValue::Number(end)) = (*start, *end) else {
                    return Err(DmnError::Feel {
                        decision: decision.into(),
                        detail: "iterator range endpoints must be numbers".into(),
                    });
                };
                if !start.is_finite()
                    || !end.is_finite()
                    || start.fract() != 0.0
                    || end.fract() != 0.0
                    || start < i64::MIN as f64
                    || start > i64::MAX as f64
                    || end < i64::MIN as f64
                    || end > i64::MAX as f64
                {
                    return Err(DmnError::Feel {
                        decision: decision.into(),
                        detail: "iterator range endpoints must be finite integers".into(),
                    });
                }
                let mut first = start as i64;
                let mut last = end as i64;
                if start_open {
                    first = first.checked_add(1).ok_or_else(|| DmnError::Feel {
                        decision: decision.into(),
                        detail: "iterator range start is out of bounds".into(),
                    })?;
                }
                if end_open {
                    last = last.checked_sub(1).ok_or_else(|| DmnError::Feel {
                        decision: decision.into(),
                        detail: "iterator range end is out of bounds".into(),
                    })?;
                }
                if first > last {
                    return Ok(Vec::new());
                }
                let count = i128::from(last) - i128::from(first) + 1;
                let count = usize::try_from(count).map_err(|_| DmnError::Feel {
                    decision: decision.into(),
                    detail: "iterator range is too large".into(),
                })?;
                let mut items = Vec::new();
                items.try_reserve(count).map_err(|_| DmnError::Feel {
                    decision: decision.into(),
                    detail: "iterator range is too large".into(),
                })?;
                for value in first..=last {
                    items.push(FeelValue::Number(value as f64));
                }
                Ok(items)
            }
            value => Err(DmnError::Feel {
                decision: decision.into(),
                detail: format!(
                    "iterator input must be a list or finite integer range, got {value}"
                ),
            }),
        }
    }

    fn index_list(items: &[FeelValue], index: f64) -> FeelValue {
        if !index.is_finite() || index.fract() != 0.0 || index == 0.0 {
            return FeelValue::Null;
        }
        if index < 0.0 {
            let distance = (-index) as usize;
            return distance
                .checked_sub(1)
                .and_then(|offset| items.len().checked_sub(offset + 1))
                .and_then(|index| items.get(index))
                .cloned()
                .unwrap_or(FeelValue::Null);
        }
        items
            .get(index as usize - 1)
            .cloned()
            .unwrap_or(FeelValue::Null)
    }

    #[cfg_attr(not(test), allow(dead_code))] // 按名称/ID 执行单个决策：预留 API，当前仅测试直接调用
    pub fn evaluate(
        &self,
        decision: &str,
        inputs: HashMap<String, FeelValue>,
    ) -> Result<DecisionResult> {
        let compiled = self.compiled()?;
        let result = if compiled.decision_indices_by_id.contains_key(decision) {
            self.evaluate_by_id(decision, inputs)
        } else if compiled.decision_ids_by_name.contains_key(decision) {
            self.evaluate_by_name(decision, inputs)
        } else {
            return Err(DmnError::UnknownDecision(decision.into()));
        };
        let target = result
            .decision_by_id(decision)
            .or_else(|| result.decision_by_name(decision));
        match target {
            Some(item) if item.status == DecisionStatus::Succeeded => Ok(item.clone()),
            _ => {
                Err(result_error(&result)
                    .unwrap_or_else(|| DmnError::UnknownDecision(decision.into())))
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))] // 单决策执行预留 API，当前仅测试直接调用
    pub fn evaluate_by_id(&self, id: &str, inputs: HashMap<String, FeelValue>) -> DmnResult {
        let compiled = match self.compiled() {
            Ok(model) => model,
            Err(error) => return failed_result(error, DmnMessageSource::default()),
        };
        if !compiled.decision_indices_by_id.contains_key(id) {
            return failed_result(
                DmnError::UnknownDecision(id.into()),
                DmnMessageSource::default(),
            );
        }
        self.evaluate_targets(compiled, std::iter::once(id), inputs)
    }

    #[cfg_attr(not(test), allow(dead_code))] // 单决策执行预留 API，当前仅测试直接调用
    pub fn evaluate_by_name(&self, name: &str, inputs: HashMap<String, FeelValue>) -> DmnResult {
        let compiled = match self.compiled() {
            Ok(model) => model,
            Err(error) => return failed_result(error, DmnMessageSource::default()),
        };
        match compiled.decision_ids_by_name.get(name) {
            Some(id) => self.evaluate_targets(compiled, std::iter::once(id.as_str()), inputs),
            None => failed_result(
                DmnError::UnknownDecision(name.into()),
                DmnMessageSource::default(),
            ),
        }
    }

    pub fn evaluate_all(&self, inputs: HashMap<String, FeelValue>) -> DmnResult {
        let compiled = match self.compiled() {
            Ok(model) => model,
            Err(error) => return failed_result(error, DmnMessageSource::default()),
        };
        self.evaluate_targets(
            compiled,
            compiled.decision_order.iter().map(String::as_str),
            inputs,
        )
    }

    #[cfg_attr(not(test), allow(dead_code))] // 决策服务执行预留 API，当前仅测试直接调用（产品用 with_details）
    pub fn evaluate_decision_service(
        &self,
        service: &str,
        inputs: HashMap<String, FeelValue>,
    ) -> DmnResult {
        let compiled = match self.compiled() {
            Ok(model) => model,
            Err(error) => return failed_result(error, DmnMessageSource::default()),
        };
        let index = compiled
            .decision_service_indices_by_id
            .get(service)
            .or_else(|| compiled.decision_service_indices_by_name.get(service));
        match index {
            Some(index) => self.evaluate_service_at(compiled, *index, inputs, true),
            None => failed_result(
                DmnError::UnknownDecisionService(service.into()),
                DmnMessageSource::default(),
            ),
        }
    }

    #[allow(dead_code)] // 编译态决策服务按 ID 执行预留 API（与 evaluate_decision_service 对称），暂无调用者（含测试）
    pub fn evaluate_decision_service_by_id(
        &self,
        id: &str,
        inputs: HashMap<String, FeelValue>,
    ) -> DmnResult {
        let compiled = match self.compiled() {
            Ok(model) => model,
            Err(error) => return failed_result(error, DmnMessageSource::default()),
        };
        match compiled.decision_service_indices_by_id.get(id) {
            Some(index) => self.evaluate_service_at(compiled, *index, inputs, true),
            None => failed_result(
                DmnError::UnknownDecisionService(id.into()),
                DmnMessageSource::default(),
            ),
        }
    }

    #[allow(dead_code)] // 编译态决策服务按名称执行预留 API（与 evaluate_decision_service 对称），暂无调用者（含测试）
    pub fn evaluate_decision_service_by_name(
        &self,
        name: &str,
        inputs: HashMap<String, FeelValue>,
    ) -> DmnResult {
        let compiled = match self.compiled() {
            Ok(model) => model,
            Err(error) => return failed_result(error, DmnMessageSource::default()),
        };
        match compiled.decision_service_indices_by_name.get(name) {
            Some(index) => self.evaluate_service_at(compiled, *index, inputs, true),
            None => failed_result(
                DmnError::UnknownDecisionService(name.into()),
                DmnMessageSource::default(),
            ),
        }
    }

    pub fn evaluate_decision_service_with_details(
        &self,
        service: &str,
        inputs: HashMap<String, FeelValue>,
    ) -> DmnResult {
        let compiled = match self.compiled() {
            Ok(model) => model,
            Err(error) => return failed_result(error, DmnMessageSource::default()),
        };
        let index = compiled
            .decision_service_indices_by_id
            .get(service)
            .or_else(|| compiled.decision_service_indices_by_name.get(service));
        match index {
            Some(index) => self.evaluate_service_at(compiled, *index, inputs, false),
            None => failed_result(
                DmnError::UnknownDecisionService(service.into()),
                DmnMessageSource::default(),
            ),
        }
    }

    fn evaluate_service_at(
        &self,
        compiled: &CompiledDmnModel<'_>,
        index: usize,
        inputs: HashMap<String, FeelValue>,
        outputs_only: bool,
    ) -> DmnResult {
        let service = &compiled.model.decision_services[index];
        if service.output_decisions.is_empty() {
            return failed_result(
                DmnError::InvalidDecisionService {
                    service: service.name.clone(),
                    detail: "decision service has no outputDecision".into(),
                },
                DmnMessageSource {
                    element_id: Some(service.id.clone()),
                    ..DmnMessageSource::default()
                },
            );
        }
        let mut seeded_decisions = HashMap::with_capacity(service.input_decisions.len());
        for id in &service.input_decisions {
            let decision = &compiled.model.decisions[compiled.decision_indices_by_id[id]];
            let key = decision.variable.as_deref().unwrap_or(&decision.name);
            let Some(value) = inputs.get(key).or_else(|| inputs.get(id)).cloned() else {
                return failed_result(
                    DmnError::MissingInput(key.into()),
                    DmnMessageSource {
                        element_id: Some(service.id.clone()),
                        ..DmnMessageSource::default()
                    },
                );
            };
            if let Err(error) =
                self.validate_declared_value(decision.type_ref.as_deref(), &value, key, compiled)
            {
                return failed_result(error, decision_source(decision));
            }
            seeded_decisions.insert(
                id.clone(),
                DecisionResult {
                    decision_id: decision.id.clone(),
                    decision_name: decision.name.clone(),
                    value,
                    matched_rule_ids: Vec::new(),
                    status: DecisionStatus::Succeeded,
                    evaluated_dependencies: Vec::new(),
                },
            );
        }
        let visible = service
            .output_decisions
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let mut result = self.evaluate_targets_with_decisions(
            compiled,
            service.output_decisions.iter().map(String::as_str),
            inputs,
            seeded_decisions,
        );
        if outputs_only {
            result
                .decision_results
                .retain(|decision| visible.contains(decision.decision_id.as_str()));
        }
        result
    }

    fn evaluate_targets<'b>(
        &self,
        compiled: &CompiledDmnModel<'_>,
        targets: impl IntoIterator<Item = &'b str>,
        inputs: HashMap<String, FeelValue>,
    ) -> DmnResult {
        self.evaluate_targets_with_decisions(compiled, targets, inputs, HashMap::new())
    }

    fn evaluate_targets_with_decisions<'b>(
        &self,
        compiled: &CompiledDmnModel<'_>,
        targets: impl IntoIterator<Item = &'b str>,
        inputs: HashMap<String, FeelValue>,
        decisions: HashMap<String, DecisionResult>,
    ) -> DmnResult {
        let mut state = ExecutionState {
            context: inputs,
            decisions,
            failure_source: None,
            validated_inputs: HashMap::new(),
        };
        let mut diagnostics = DmnDiagnostics::new();
        for target in targets {
            self.evaluate_node(target, compiled, &mut state, &mut diagnostics);
        }
        let decision_results = compiled
            .decision_order
            .iter()
            .filter_map(|id| state.decisions.remove(id))
            .collect::<Vec<_>>();
        result_from(decision_results, diagnostics)
    }

    fn evaluate_node(
        &self,
        id: &str,
        compiled: &CompiledDmnModel<'_>,
        state: &mut ExecutionState,
        diagnostics: &mut DmnDiagnostics,
    ) {
        if state.decisions.contains_key(id) {
            return;
        }
        let decision = match compiled.decision_indices_by_id.get(id) {
            Some(index) => &compiled.model.decisions[*index],
            None => return,
        };
        let source = decision_source(decision);
        let mut dependencies = Vec::new();
        let mut dependency_failed = false;
        for requirement in &decision.requirements {
            if let Some(input_id) = &requirement.required_input {
                let input = &compiled.model.input_data[compiled.input_indices_by_id[input_id]];
                let key = input.variable.as_deref().unwrap_or(&input.name);
                if !state.context.contains_key(key) {
                    dependency_failed = true;
                    diagnostics.error(DmnError::MissingInput(key.into()), source.clone());
                } else if let Err(error) = self.validate_input(input, key, compiled, state) {
                    dependency_failed = true;
                    diagnostics.error(error, source.clone());
                }
            }
            if let Some(required) = &requirement.required_decision {
                self.evaluate_node(required, compiled, state, diagnostics);
                dependencies.push(required.clone());
                match state.decisions.get(required) {
                    Some(result) if result.status == DecisionStatus::Succeeded => {
                        let dependency =
                            &compiled.model.decisions[compiled.decision_indices_by_id[required]];
                        state.context.insert(
                            dependency
                                .variable
                                .as_deref()
                                .unwrap_or(&dependency.name)
                                .into(),
                            result.value.clone(),
                        );
                    }
                    _ => dependency_failed = true,
                }
            }
        }
        if dependency_failed {
            diagnostics.dependency_failure(source.clone());
            state.decisions.insert(
                id.into(),
                DecisionResult {
                    decision_id: decision.id.clone(),
                    decision_name: decision.name.clone(),
                    value: FeelValue::Null,
                    matched_rule_ids: Vec::new(),
                    status: DecisionStatus::Skipped,
                    evaluated_dependencies: dependencies,
                },
            );
            return;
        }
        let mut knowledge_bindings = Vec::new();
        let evaluation = self
            .bind_knowledge_requirements(decision, state, compiled, &mut knowledge_bindings)
            .and_then(|_| self.evaluate_logic(decision, state, compiled))
            .and_then(|table| {
                self.validate_decision_value(
                    decision.type_ref.as_deref(),
                    &table.value,
                    &decision.name,
                    compiled,
                )?;
                Ok(table)
            });
        Self::restore_context_bindings(state, knowledge_bindings);
        match evaluation {
            Ok(table) => {
                state.decisions.insert(
                    id.into(),
                    DecisionResult {
                        decision_id: decision.id.clone(),
                        decision_name: decision.name.clone(),
                        value: table.value,
                        matched_rule_ids: table.matched_rule_ids,
                        status: DecisionStatus::Succeeded,
                        evaluated_dependencies: dependencies,
                    },
                );
            }
            Err(error) => {
                let error_source =
                    state
                        .failure_source
                        .take()
                        .unwrap_or_else(|| match &decision.logic {
                            DecisionLogic::DecisionTable(table) => {
                                decision_table_source(decision, table)
                            }
                            _ => source,
                        });
                diagnostics.error(error, error_source);
                state.decisions.insert(
                    id.into(),
                    DecisionResult {
                        decision_id: decision.id.clone(),
                        decision_name: decision.name.clone(),
                        value: FeelValue::Null,
                        matched_rule_ids: Vec::new(),
                        status: DecisionStatus::Failed,
                        evaluated_dependencies: dependencies,
                    },
                );
            }
        }
    }

    fn bind_knowledge_requirements(
        &self,
        decision: &Decision,
        state: &mut ExecutionState,
        compiled: &CompiledDmnModel<'_>,
        bindings: &mut Vec<(String, Option<FeelValue>)>,
    ) -> Result<()> {
        let mut visiting = HashSet::new();
        for requirement in &decision.knowledge_requirements {
            let index = compiled.bkm_indices_by_id[&requirement.required_knowledge];
            let bkm = &compiled.model.business_knowledge_models[index];
            if bkm_has_literal_feel_body(bkm) {
                self.bind_bkm_function(
                    &requirement.required_knowledge,
                    state,
                    compiled,
                    &mut visiting,
                    bindings,
                )?;
            }
        }
        Ok(())
    }

    fn bind_bkm_function(
        &self,
        bkm_id: &str,
        state: &mut ExecutionState,
        compiled: &CompiledDmnModel<'_>,
        visiting: &mut HashSet<String>,
        bindings: &mut Vec<(String, Option<FeelValue>)>,
    ) -> Result<()> {
        let index =
            compiled
                .bkm_indices_by_id
                .get(bkm_id)
                .ok_or_else(|| DmnError::UnknownReference {
                    decision: "knowledge requirement".into(),
                    reference: bkm_id.into(),
                })?;
        let bkm = &compiled.model.business_knowledge_models[*index];
        let binding_name = bkm.variable.as_deref().unwrap_or(&bkm.name);
        if state.context.contains_key(binding_name) {
            return Ok(());
        }
        if !visiting.insert(bkm.id.clone()) {
            return Err(DmnError::Feel {
                decision: bkm.name.clone(),
                detail: "cyclic business knowledge model dependency".into(),
            });
        }
        for requirement in &bkm.knowledge_requirements {
            let dependency = &compiled.model.business_knowledge_models
                [compiled.bkm_indices_by_id[&requirement.required_knowledge]];
            if bkm_has_literal_feel_body(dependency) {
                self.bind_bkm_function(
                    &requirement.required_knowledge,
                    state,
                    compiled,
                    visiting,
                    bindings,
                )?;
            }
        }
        let DecisionLogic::FunctionDefinition(function) = &bkm.logic else {
            return Err(DmnError::UnsupportedFeature {
                decision: bkm.name.clone(),
                feature: "business knowledge model without a function definition".into(),
            });
        };
        let DecisionLogic::LiteralExpression(body) = function.body.as_ref() else {
            return Err(DmnError::UnsupportedFeature {
                decision: bkm.name.clone(),
                feature: "direct FEEL invocation of a non-literal BKM body".into(),
            });
        };
        let body = compiled
            .feel_expressions
            .get(body)
            .cloned()
            .ok_or_else(|| DmnError::Feel {
                decision: bkm.name.clone(),
                detail: "business knowledge model body was not compiled".into(),
            })?;
        let function_value = FeelValue::Function(function.parameters.clone(), body);
        let previous = state
            .context
            .insert(binding_name.into(), function_value.clone());
        bindings.push((binding_name.into(), previous));
        if binding_name != bkm.name {
            let previous = state.context.insert(bkm.name.clone(), function_value);
            bindings.push((bkm.name.clone(), previous));
        }
        visiting.remove(&bkm.id);
        Ok(())
    }

    fn validate_input(
        &self,
        input: &InputData,
        key: &str,
        compiled: &CompiledDmnModel<'_>,
        state: &mut ExecutionState,
    ) -> Result<()> {
        if let Some(result) = state.validated_inputs.get(&input.id) {
            return result.clone();
        }
        let result = self.validate_declared_value(
            input.type_ref.as_deref(),
            state.context.get(key).expect("input presence was checked"),
            key,
            compiled,
        );
        state
            .validated_inputs
            .insert(input.id.clone(), result.clone());
        result
    }

    fn validate_declared_value(
        &self,
        type_ref: Option<&str>,
        value: &FeelValue,
        location: &str,
        compiled: &CompiledDmnModel<'_>,
    ) -> Result<()> {
        let Some(type_ref) = type_ref else {
            return Ok(());
        };
        if let Some(item_type) = compiled.item_types.get(type_ref) {
            return self.validate_item_type(item_type, value, location, compiled);
        }
        self.validate_builtin_type(type_ref, value, location)
    }

    // A decision may intentionally produce null when its business rule is
    // incomplete or no rule matches. Null is absence of a decision result,
    // rather than an enum member, so keep strict type checks for inputs while
    // allowing this value through decision-output paths.
    fn validate_decision_value(
        &self,
        type_ref: Option<&str>,
        value: &FeelValue,
        location: &str,
        compiled: &CompiledDmnModel<'_>,
    ) -> Result<()> {
        if matches!(value, FeelValue::Null) {
            return Ok(());
        }
        self.validate_declared_value(type_ref, value, location, compiled)
    }

    fn validate_item_type(
        &self,
        item_type: &CompiledType,
        value: &FeelValue,
        location: &str,
        compiled: &CompiledDmnModel<'_>,
    ) -> Result<()> {
        if matches!(value, FeelValue::Null) {
            return self.validate_allowed_values(item_type, value, location, compiled);
        }
        if item_type.is_collection {
            let FeelValue::List(values) = value else {
                return Err(type_violation(location, &item_type.name, "expected a list"));
            };
            for (index, value) in values.iter().enumerate() {
                self.validate_item_type_value(
                    item_type,
                    value,
                    &format!("{location}[{}]", index + 1),
                    compiled,
                )?;
            }
            return Ok(());
        }
        self.validate_item_type_value(item_type, value, location, compiled)
    }

    fn validate_item_type_value(
        &self,
        item_type: &CompiledType,
        value: &FeelValue,
        location: &str,
        compiled: &CompiledDmnModel<'_>,
    ) -> Result<()> {
        if let Some(type_ref) = &item_type.type_ref {
            self.validate_declared_value(Some(type_ref), value, location, compiled)?;
        }
        if !item_type.components.is_empty() {
            let FeelValue::Context(context) = value else {
                return Err(type_violation(
                    location,
                    &item_type.name,
                    "expected a context",
                ));
            };
            for component in &item_type.components {
                let component_value = context.get(&component.name).ok_or_else(|| {
                    type_violation(
                        location,
                        &item_type.name,
                        &format!("missing component '{}'", component.name),
                    )
                })?;
                self.validate_item_type(
                    component,
                    component_value,
                    &format!("{location}.{}", component.name),
                    compiled,
                )?;
            }
        }
        self.validate_allowed_values(item_type, value, location, compiled)
    }

    fn validate_allowed_values(
        &self,
        item_type: &CompiledType,
        value: &FeelValue,
        location: &str,
        compiled: &CompiledDmnModel<'_>,
    ) -> Result<()> {
        if let Some(allowed_values) = &item_type.allowed_values {
            let mut context = HashMap::new();
            let allowed = self
                .eval_unary_tests(
                    compiled,
                    allowed_values,
                    value,
                    &mut context,
                    &item_type.name,
                )
                .map_err(|error| type_violation(location, &item_type.name, &error.to_string()))?;
            if !allowed {
                return Err(type_violation(
                    location,
                    &item_type.name,
                    "value is outside allowedValues",
                ));
            }
        }
        Ok(())
    }

    fn validate_builtin_type(
        &self,
        type_ref: &str,
        value: &FeelValue,
        location: &str,
    ) -> Result<()> {
        if matches!(value, FeelValue::Null) {
            return Ok(());
        }
        let type_ref = type_ref
            .strip_prefix("feel:")
            .or_else(|| type_ref.strip_prefix("feel."))
            .unwrap_or(type_ref);
        // FEEL's Any type accepts every non-null FEEL value. Check this after
        // normalizing namespace prefixes so feel:Any and feel.Any work too.
        if matches!(type_ref, "Any" | "any") {
            return Ok(());
        }
        let valid = matches!(
            (type_ref, value),
            ("number", FeelValue::Number(_))
                | ("string", FeelValue::String(_))
                | ("boolean", FeelValue::Bool(_))
                | ("date", FeelValue::Date(_))
                | ("time", FeelValue::Time(_))
                | ("date and time" | "dateTime", FeelValue::DateTime(_))
                | (
                    "duration"
                        | "yearMonthDuration"
                        | "dayTimeDuration"
                        | "years and months duration"
                        | "days and time duration",
                    FeelValue::Duration(_)
                )
                | ("context", FeelValue::Context(_))
                | ("list", FeelValue::List(_))
                | ("function", FeelValue::Function(_, _))
        );
        if valid {
            Ok(())
        } else {
            Err(type_violation(
                location,
                type_ref,
                "value has an incompatible FEEL type",
            ))
        }
    }

    fn evaluate_logic(
        &self,
        decision: &Decision,
        state: &mut ExecutionState,
        compiled: &CompiledDmnModel<'_>,
    ) -> Result<TableEvaluation> {
        self.evaluate_logic_inner(decision, &decision.logic, state, compiled)
    }

    fn evaluate_logic_inner(
        &self,
        decision: &Decision,
        logic: &DecisionLogic,
        state: &mut ExecutionState,
        compiled: &CompiledDmnModel<'_>,
    ) -> Result<TableEvaluation> {
        match logic {
            DecisionLogic::LiteralExpression(expression) => self
                .eval_expression(compiled, expression, &state.context, &decision.name)
                .map(|value| TableEvaluation {
                    value,
                    matched_rule_ids: Vec::new(),
                }),
            DecisionLogic::DecisionTable(table) => {
                self.evaluate_table(decision, table, state, compiled)
            }
            DecisionLogic::Context(entries) => {
                let mut context = HashMap::new();
                let mut previous = Vec::with_capacity(entries.len());
                let evaluated = (|| {
                    let mut result = None;
                    for entry in entries {
                        let value = self
                            .evaluate_logic_inner(decision, &entry.expression, state, compiled)?
                            .value;
                        self.validate_declared_value(
                            entry.type_ref.as_deref(),
                            &value,
                            &format!(
                                "{}.{}",
                                decision.name,
                                if entry.name.is_empty() {
                                    "context result"
                                } else {
                                    &entry.name
                                }
                            ),
                            compiled,
                        )?;
                        if entry.name.is_empty() {
                            result = Some(value);
                        } else {
                            previous.push((
                                entry.name.clone(),
                                state.context.insert(entry.name.clone(), value.clone()),
                            ));
                            context.insert(entry.name.clone(), value);
                        }
                    }
                    Ok(result.unwrap_or(FeelValue::Context(context)))
                })();
                for (name, old_value) in previous.into_iter().rev() {
                    if let Some(old_value) = old_value {
                        state.context.insert(name, old_value);
                    } else {
                        state.context.remove(&name);
                    }
                }
                Ok(TableEvaluation {
                    value: evaluated?,
                    matched_rule_ids: Vec::new(),
                })
            }
            DecisionLogic::List(expressions) => Ok(TableEvaluation {
                value: FeelValue::List(
                    expressions
                        .iter()
                        .map(|expression| {
                            self.evaluate_logic_inner(decision, expression, state, compiled)
                                .map(|result| result.value)
                        })
                        .collect::<Result<Vec<_>>>()?,
                ),
                matched_rule_ids: Vec::new(),
            }),
            DecisionLogic::Relation(relation) => {
                let mut rows = Vec::with_capacity(relation.rows.len());
                for (row_index, row) in relation.rows.iter().enumerate() {
                    let mut context = HashMap::with_capacity(relation.columns.len());
                    for (column, entry) in relation.columns.iter().zip(&row.entries) {
                        let value = self
                            .evaluate_logic_inner(decision, entry, state, compiled)?
                            .value;
                        self.validate_declared_value(
                            column.type_ref.as_deref(),
                            &value,
                            &format!(
                                "{}.relation[{}].{}",
                                decision.name,
                                row_index + 1,
                                column.name
                            ),
                            compiled,
                        )?;
                        context.insert(column.name.clone(), value);
                    }
                    rows.push(FeelValue::Context(context));
                }
                Ok(TableEvaluation {
                    value: FeelValue::List(rows),
                    matched_rule_ids: Vec::new(),
                })
            }
            DecisionLogic::Conditional(conditional) => {
                let condition = self
                    .evaluate_logic_inner(decision, &conditional.condition, state, compiled)?
                    .value;
                let branch = if condition == FeelValue::Bool(true) {
                    &conditional.then_expression
                } else {
                    &conditional.else_expression
                };
                self.evaluate_logic_inner(decision, branch, state, compiled)
            }
            DecisionLogic::Filter(filter) => {
                let input = self
                    .evaluate_logic_inner(decision, &filter.input, state, compiled)?
                    .value;
                let FeelValue::List(items) = input else {
                    return Err(DmnError::Feel {
                        decision: decision.name.clone(),
                        detail: format!("filter input must be a list, got {input}"),
                    });
                };

                if let Ok(TableEvaluation {
                    value: FeelValue::Number(index),
                    ..
                }) = self.evaluate_logic_inner(decision, &filter.matcher, state, compiled)
                {
                    return Ok(TableEvaluation {
                        value: Self::index_list(&items, index),
                        matched_rule_ids: Vec::new(),
                    });
                }

                let mut filtered = Vec::new();
                for item in items {
                    let mut bindings = match &item {
                        FeelValue::Context(entries) => entries
                            .iter()
                            .map(|(name, value)| (name.clone(), value.clone()))
                            .collect::<Vec<_>>(),
                        _ => Vec::new(),
                    };
                    if !bindings.iter().any(|(name, _)| name == "item") {
                        bindings.push(("item".into(), item.clone()));
                    }
                    let matches = Self::with_context_bindings(state, bindings, |state| {
                        self.evaluate_logic_inner(decision, &filter.matcher, state, compiled)
                    })?
                    .value
                        == FeelValue::Bool(true);
                    if matches {
                        filtered.push(item);
                    }
                }
                Ok(TableEvaluation {
                    value: FeelValue::List(filtered),
                    matched_rule_ids: Vec::new(),
                })
            }
            DecisionLogic::For(iterator) => {
                let input = self
                    .evaluate_logic_inner(decision, &iterator.input, state, compiled)?
                    .value;
                let items = Self::expand_iterable(input, &decision.name)?;
                let uses_partial = logic_mentions_name(&iterator.return_expression, "partial");
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    let mut bindings = vec![(iterator.variable.clone(), item)];
                    if uses_partial && iterator.variable != "partial" {
                        bindings.push(("partial".into(), FeelValue::List(values.clone())));
                    }
                    let value = Self::with_context_bindings(state, bindings, |state| {
                        self.evaluate_logic_inner(
                            decision,
                            &iterator.return_expression,
                            state,
                            compiled,
                        )
                    })?
                    .value;
                    values.push(value);
                }
                Ok(TableEvaluation {
                    value: FeelValue::List(values),
                    matched_rule_ids: Vec::new(),
                })
            }
            DecisionLogic::Some(quantified) | DecisionLogic::Every(quantified) => {
                let input = self
                    .evaluate_logic_inner(decision, &quantified.input, state, compiled)?
                    .value;
                let items = Self::expand_iterable(input, &decision.name)?;
                let is_some = matches!(logic, DecisionLogic::Some(_));
                let mut saw_unknown = false;
                for item in items {
                    let value = Self::with_context_bindings(
                        state,
                        vec![(quantified.variable.clone(), item)],
                        |state| {
                            self.evaluate_logic_inner(
                                decision,
                                &quantified.satisfies,
                                state,
                                compiled,
                            )
                        },
                    )?
                    .value;
                    match value {
                        FeelValue::Bool(true) if is_some => {
                            return Ok(TableEvaluation {
                                value: FeelValue::Bool(true),
                                matched_rule_ids: Vec::new(),
                            });
                        }
                        FeelValue::Bool(false) if !is_some => {
                            return Ok(TableEvaluation {
                                value: FeelValue::Bool(false),
                                matched_rule_ids: Vec::new(),
                            });
                        }
                        FeelValue::Bool(_) => {}
                        FeelValue::Null => saw_unknown = true,
                        _ => saw_unknown = true,
                    }
                }
                Ok(TableEvaluation {
                    value: if saw_unknown {
                        FeelValue::Null
                    } else {
                        FeelValue::Bool(!is_some)
                    },
                    matched_rule_ids: Vec::new(),
                })
            }
            DecisionLogic::FunctionDefinition(function) => {
                let DecisionLogic::LiteralExpression(body) = function.body.as_ref() else {
                    return Err(DmnError::UnsupportedFeature {
                        decision: decision.name.clone(),
                        feature: "non-literal function body".into(),
                    });
                };
                let body = compiled
                    .feel_expressions
                    .get(body)
                    .cloned()
                    .ok_or_else(|| DmnError::Feel {
                        decision: decision.name.clone(),
                        detail: "function body was not compiled".into(),
                    })?;
                Ok(TableEvaluation {
                    value: FeelValue::Function(function.parameters.clone(), body),
                    matched_rule_ids: Vec::new(),
                })
            }
            DecisionLogic::Invocation(invocation) => {
                let DecisionLogic::LiteralExpression(target) = invocation.expression.as_ref()
                else {
                    return Err(DmnError::UnsupportedFeature {
                        decision: decision.name.clone(),
                        feature: "non-literal invocation target".into(),
                    });
                };
                let mut evaluated_bindings = Vec::with_capacity(invocation.bindings.len());
                let mut binding_names = HashSet::with_capacity(invocation.bindings.len());
                for binding in &invocation.bindings {
                    let value = self
                        .evaluate_logic_inner(decision, &binding.expression, state, compiled)?
                        .value;
                    binding_names.insert(binding.parameter.as_str());
                    evaluated_bindings.push((binding.parameter.clone(), value));
                }
                let target = target.trim();
                if let Some(index) = compiled.bkm_indices_by_name.get(target) {
                    let bkm = &compiled.model.business_knowledge_models[*index];
                    let mut bkm_state = ExecutionState {
                        context: evaluated_bindings.iter().cloned().collect(),
                        decisions: HashMap::new(),
                        failure_source: None,
                        validated_inputs: HashMap::new(),
                    };
                    let mut visiting = HashSet::new();
                    let mut knowledge_bindings = Vec::new();
                    for requirement in &bkm.knowledge_requirements {
                        let dependency = &compiled.model.business_knowledge_models
                            [compiled.bkm_indices_by_id[&requirement.required_knowledge]];
                        if bkm_has_literal_feel_body(dependency) {
                            self.bind_bkm_function(
                                &requirement.required_knowledge,
                                &mut bkm_state,
                                compiled,
                                &mut visiting,
                                &mut knowledge_bindings,
                            )?;
                        }
                    }
                    if let DecisionLogic::FunctionDefinition(function) = &bkm.logic {
                        for (parameter_index, parameter) in function.parameters.iter().enumerate() {
                            // Some generated models omit an explicit binding
                            // and rely on a same-named declared input from the
                            // surrounding decision context. Accept only that
                            // unambiguous shorthand; undeclared names remain
                            // strict errors.
                            let declared_input = compiled.model.input_data.iter().any(|input| {
                                input.name == *parameter
                                    || input.variable.as_deref() == Some(parameter.as_str())
                            });
                            if !binding_names.contains(parameter.as_str())
                                && declared_input
                                && let Some(value) = state.context.get(parameter).cloned()
                            {
                                bkm_state.context.insert(parameter.clone(), value);
                                binding_names.insert(parameter.as_str());
                            }
                            if !binding_names.contains(parameter.as_str()) {
                                return Err(DmnError::Feel {
                                    decision: decision.name.clone(),
                                    detail: format!("missing invocation binding '{parameter}'"),
                                });
                            }
                            if let Some(Some(type_ref)) =
                                function.parameter_type_refs.get(parameter_index)
                            {
                                self.validate_declared_value(
                                    Some(type_ref),
                                    bkm_state
                                        .context
                                        .get(parameter)
                                        .expect("binding presence was checked"),
                                    &format!("{}.{}", bkm.name, parameter),
                                    compiled,
                                )?;
                            }
                        }
                        if let Some(binding) = invocation
                            .bindings
                            .iter()
                            .find(|binding| !function.parameters.contains(&binding.parameter))
                        {
                            return Err(DmnError::Feel {
                                decision: decision.name.clone(),
                                detail: format!(
                                    "unknown invocation binding '{}' for '{}'",
                                    binding.parameter, bkm.name
                                ),
                            });
                        }
                        let body =
                            unwrap_redundant_function_body(&function.body, &bkm_state.context);
                        let result =
                            self.evaluate_logic_inner(decision, body, &mut bkm_state, compiled)?;
                        self.validate_declared_value(
                            function.result_type_ref.as_deref(),
                            &result.value,
                            &format!("{}.result", bkm.name),
                            compiled,
                        )?;
                        if bkm.type_ref != function.result_type_ref {
                            self.validate_declared_value(
                                bkm.type_ref.as_deref(),
                                &result.value,
                                &format!("{}.result", bkm.name),
                                compiled,
                            )?;
                        }
                        return Ok(result);
                    }
                    let result =
                        self.evaluate_logic_inner(decision, &bkm.logic, &mut bkm_state, compiled)?;
                    if let FeelValue::Function(parameters, body) = result.value {
                        let function_context = bkm_state.context;
                        for parameter in &parameters {
                            if !function_context.contains_key(parameter) {
                                return Err(DmnError::Feel {
                                    decision: decision.name.clone(),
                                    detail: format!("missing invocation binding '{parameter}'"),
                                });
                            }
                        }
                        let value = FeelInterpreter::new()
                            .eval_expr(&body, &function_context)
                            .map_err(|detail| DmnError::Feel {
                                decision: decision.name.clone(),
                                detail,
                            })?;
                        self.validate_declared_value(
                            bkm.type_ref.as_deref(),
                            &value,
                            &format!("{}.result", bkm.name),
                            compiled,
                        )?;
                        return Ok(TableEvaluation {
                            value,
                            matched_rule_ids: Vec::new(),
                        });
                    }
                    self.validate_declared_value(
                        bkm.type_ref.as_deref(),
                        &result.value,
                        &format!("{}.result", bkm.name),
                        compiled,
                    )?;
                    return Ok(result);
                }
                let mut context = state.context.clone();
                context.extend(evaluated_bindings);
                let target_value =
                    self.eval_expression(compiled, target, &context, &decision.name)?;
                if let FeelValue::Function(parameters, body) = target_value {
                    for parameter in &parameters {
                        let declared_input = compiled.model.input_data.iter().any(|input| {
                            input.name == *parameter
                                || input.variable.as_deref() == Some(parameter.as_str())
                        });
                        if !binding_names.contains(parameter.as_str())
                            && declared_input
                            && state.context.contains_key(parameter)
                        {
                            binding_names.insert(parameter.as_str());
                        }
                        if !binding_names.contains(parameter.as_str()) {
                            return Err(DmnError::Feel {
                                decision: decision.name.clone(),
                                detail: format!("missing invocation binding '{parameter}'"),
                            });
                        }
                    }
                    if let Some(binding) = invocation
                        .bindings
                        .iter()
                        .find(|binding| !parameters.contains(&binding.parameter))
                    {
                        return Err(DmnError::Feel {
                            decision: decision.name.clone(),
                            detail: format!("unknown invocation binding '{}'", binding.parameter),
                        });
                    }
                    let value =
                        FeelInterpreter::new()
                            .eval_expr(&body, &context)
                            .map_err(|detail| DmnError::Feel {
                                decision: decision.name.clone(),
                                detail,
                            })?;
                    return Ok(TableEvaluation {
                        value,
                        matched_rule_ids: Vec::new(),
                    });
                }
                Ok(TableEvaluation {
                    value: target_value,
                    matched_rule_ids: Vec::new(),
                })
            }
            DecisionLogic::Unsupported(feature) => Err(DmnError::UnsupportedFeature {
                decision: decision.name.clone(),
                feature: feature.clone(),
            }),
            DecisionLogic::None => Err(DmnError::UnsupportedFeature {
                decision: decision.name.clone(),
                feature: "no decision logic".into(),
            }),
        }
    }

    fn evaluate_table(
        &self,
        decision: &Decision,
        table: &DecisionTable,
        state: &mut ExecutionState,
        compiled: &CompiledDmnModel<'_>,
    ) -> Result<TableEvaluation> {
        let values: Vec<FeelValue> = table
            .inputs
            .iter()
            .map(|input| {
                let value = self.eval_expression(
                    compiled,
                    &input.expression,
                    &state.context,
                    &decision.name,
                )?;
                self.validate_declared_value(
                    input.type_ref.as_deref(),
                    &value,
                    &format!(
                        "{}.{}",
                        decision.name,
                        input.variable.as_deref().unwrap_or("input")
                    ),
                    compiled,
                )?;
                Ok(value)
            })
            .collect::<Result<_>>()?;
        let input_allowed = table
            .inputs
            .iter()
            .zip(&values)
            .map(|(input, value)| match &input.input_values {
                Some(input_values) => self.eval_unary_tests(
                    compiled,
                    input_values,
                    value,
                    &mut state.context,
                    &decision.name,
                ),
                None => Ok(true),
            })
            .collect::<Result<Vec<_>>>()?;
        let mut matched: Vec<&DecisionRule> = Vec::new();
        for rule in &table.rules {
            if rule.input_entries.len() != values.len()
                || rule.output_entries.len() != table.outputs.len()
            {
                return Err(DmnError::InvalidDecisionTable {
                    decision: decision.name.clone(),
                    detail: "rule entry count does not match clauses".into(),
                });
            }
            let mut applies = true;
            for (((test, value), input), allowed) in rule
                .input_entries
                .iter()
                .zip(&values)
                .zip(&table.inputs)
                .zip(&input_allowed)
            {
                if !allowed {
                    applies = false;
                    break;
                }
                let test = test.trim();
                let test_applies = if test.is_empty() || test == "-" {
                    Ok(input.input_values.is_some() || !matches!(value, FeelValue::Null))
                } else {
                    self.eval_unary_tests(compiled, test, value, &mut state.context, &decision.name)
                }
                .inspect_err(|_| {
                    state.failure_source = Some(rule_source(decision, table, rule));
                })?;
                if !test_applies {
                    applies = false;
                    break;
                }
            }
            if applies {
                matched.push(rule);
                if table.hit_policy == HitPolicy::First {
                    break;
                }
            }
        }
        let matched_rule_ids = matched
            .iter()
            .map(|rule| {
                rule.id.clone().unwrap_or_else(|| {
                    let index = table
                        .rules
                        .iter()
                        .position(|candidate| std::ptr::eq(candidate, *rule))
                        .expect("matched rule belongs to the decision table");
                    format!("rule-{}", index + 1)
                })
            })
            .collect();
        if matched.is_empty() {
            return Ok(TableEvaluation {
                value: self.default_output(decision, table, &mut state.context, compiled)?,
                matched_rule_ids,
            });
        }
        let value = match table.hit_policy {
            HitPolicy::Unique => {
                if matched.len() > 1 {
                    Err(DmnError::HitPolicyViolation {
                        decision: decision.name.clone(),
                        detail: "UNIQUE matched more than one rule".into(),
                    })
                } else {
                    self.output_value(decision, table, matched[0], &mut state.context, compiled)
                        .inspect_err(|_| {
                            state.failure_source = Some(rule_source(decision, table, matched[0]));
                        })
                }
            }
            HitPolicy::First => self
                .output_value(decision, table, matched[0], &mut state.context, compiled)
                .inspect_err(|_| {
                    state.failure_source = Some(rule_source(decision, table, matched[0]));
                }),
            HitPolicy::Any => {
                let outputs = self.output_values(decision, table, &matched, state, compiled)?;
                if outputs.windows(2).any(|pair| pair[0] != pair[1]) {
                    Err(DmnError::HitPolicyViolation {
                        decision: decision.name.clone(),
                        detail: "ANY matched unequal outputs".into(),
                    })
                } else {
                    Ok(outputs
                        .into_iter()
                        .next()
                        .expect("matched rules have outputs"))
                }
            }
            HitPolicy::Priority => self.priority(decision, table, &matched, state, compiled),
            HitPolicy::RuleOrder => self
                .output_values(decision, table, &matched, state, compiled)
                .map(FeelValue::List),
            HitPolicy::OutputOrder => {
                let outputs = self.output_values(decision, table, &matched, state, compiled)?;
                self.output_order(decision, table, outputs, state, compiled)
            }
            HitPolicy::Collect => {
                let outputs = self.output_values(decision, table, &matched, state, compiled)?;
                self.collect(decision, table, outputs)
            }
        }?;
        Ok(TableEvaluation {
            value,
            matched_rule_ids,
        })
    }

    fn output_values(
        &self,
        decision: &Decision,
        table: &DecisionTable,
        rules: &[&DecisionRule],
        state: &mut ExecutionState,
        compiled: &CompiledDmnModel<'_>,
    ) -> Result<Vec<FeelValue>> {
        rules
            .iter()
            .map(|rule| {
                self.output_value(decision, table, rule, &mut state.context, compiled)
                    .inspect_err(|_| {
                        state.failure_source = Some(rule_source(decision, table, rule));
                    })
            })
            .collect()
    }
    fn output_value(
        &self,
        decision: &Decision,
        table: &DecisionTable,
        rule: &DecisionRule,
        context: &mut HashMap<String, FeelValue>,
        compiled: &CompiledDmnModel<'_>,
    ) -> Result<FeelValue> {
        let values = rule
            .output_entries
            .iter()
            .zip(&table.outputs)
            .map(|(entry, output)| {
                let value = self.eval_expression(compiled, entry, context, &decision.name)?;
                self.validate_decision_value(
                    output.type_ref.as_deref(),
                    &value,
                    &format!(
                        "{}.{}",
                        decision.name,
                        output.name.as_deref().unwrap_or("output")
                    ),
                    compiled,
                )?;
                if !matches!(value, FeelValue::Null)
                    && !matches!(
                        table.hit_policy,
                        HitPolicy::Priority | HitPolicy::OutputOrder
                    )
                {
                    self.validate_output_values(decision, output, &value, context, compiled)?;
                }
                Ok(value)
            })
            .collect::<Result<Vec<_>>>()?;
        if values.len() == 1 {
            Ok(values.into_iter().next().unwrap())
        } else {
            let mut result = HashMap::new();
            for (output, value) in table.outputs.iter().zip(values) {
                result.insert(
                    output
                        .name
                        .clone()
                        .ok_or_else(|| DmnError::InvalidDecisionTable {
                            decision: decision.name.clone(),
                            detail: "multi-output table has unnamed output".into(),
                        })?,
                    value,
                );
            }
            Ok(FeelValue::Context(result))
        }
    }
    fn default_output(
        &self,
        decision: &Decision,
        table: &DecisionTable,
        context: &mut HashMap<String, FeelValue>,
        compiled: &CompiledDmnModel<'_>,
    ) -> Result<FeelValue> {
        if table.outputs.len() == 1 {
            let output = &table.outputs[0];
            let value = match &output.default_output_entry {
                Some(entry) => self.eval_expression(compiled, entry, context, &decision.name),
                None => Ok(FeelValue::Null),
            }?;
            self.validate_decision_value(
                output.type_ref.as_deref(),
                &value,
                &format!(
                    "{}.{}",
                    decision.name,
                    output.name.as_deref().unwrap_or("output")
                ),
                compiled,
            )?;
            if !matches!(value, FeelValue::Null) {
                self.validate_output_values(decision, output, &value, context, compiled)?;
            }
            return Ok(value);
        }
        if table
            .outputs
            .iter()
            .all(|output| output.default_output_entry.is_none())
        {
            return Ok(FeelValue::Null);
        }
        let mut result = HashMap::new();
        for output in &table.outputs {
            let value = match &output.default_output_entry {
                Some(entry) => self.eval_expression(compiled, entry, context, &decision.name)?,
                None => FeelValue::Null,
            };
            self.validate_decision_value(
                output.type_ref.as_deref(),
                &value,
                &format!(
                    "{}.{}",
                    decision.name,
                    output.name.as_deref().unwrap_or("output")
                ),
                compiled,
            )?;
            if !matches!(value, FeelValue::Null) {
                self.validate_output_values(decision, output, &value, context, compiled)?;
            }
            let name = output
                .name
                .clone()
                .ok_or_else(|| DmnError::InvalidDecisionTable {
                    decision: decision.name.clone(),
                    detail: "multi-output table has unnamed output".into(),
                })?;
            result.insert(name, value);
        }
        Ok(FeelValue::Context(result))
    }

    fn validate_output_values(
        &self,
        decision: &Decision,
        output: &OutputClause,
        value: &FeelValue,
        context: &mut HashMap<String, FeelValue>,
        compiled: &CompiledDmnModel<'_>,
    ) -> Result<()> {
        let Some(output_values) = &output.output_values else {
            return Ok(());
        };
        for test in output_values {
            if self.eval_unary_tests(compiled, test, value, context, &decision.name)? {
                return Ok(());
            }
        }
        Err(type_violation(
            &format!(
                "{}.{}",
                decision.name,
                output.name.as_deref().unwrap_or("output")
            ),
            "outputValues",
            "value is outside outputValues",
        ))
    }

    fn priority(
        &self,
        decision: &Decision,
        table: &DecisionTable,
        rules: &[&DecisionRule],
        state: &mut ExecutionState,
        compiled: &CompiledDmnModel<'_>,
    ) -> Result<FeelValue> {
        let mut selected: Option<(Vec<usize>, FeelValue)> = None;
        for rule in rules {
            let value = self
                .output_value(decision, table, rule, &mut state.context, compiled)
                .inspect_err(|_| {
                    state.failure_source = Some(rule_source(decision, table, rule));
                })?;
            let key = self.priority_key(decision, table, &value, state, compiled)?;
            if selected
                .as_ref()
                .is_none_or(|(selected_key, _)| key.cmp(selected_key) == Ordering::Less)
            {
                selected = Some((key, value));
            }
        }
        Ok(selected.expect("PRIORITY has at least one matched rule").1)
    }

    fn priority_key(
        &self,
        decision: &Decision,
        table: &DecisionTable,
        value: &FeelValue,
        state: &mut ExecutionState,
        compiled: &CompiledDmnModel<'_>,
    ) -> Result<Vec<usize>> {
        let mut key = Vec::new();
        for output in &table.outputs {
            let Some(priorities) = &output.output_values else {
                continue;
            };
            let component = if table.outputs.len() == 1 {
                value
            } else {
                let name = output
                    .name
                    .as_ref()
                    .ok_or_else(|| DmnError::InvalidDecisionTable {
                        decision: decision.name.clone(),
                        detail: "multi-output table has unnamed output".into(),
                    })?;
                let FeelValue::Context(context) = value else {
                    return Err(DmnError::HitPolicyViolation {
                        decision: decision.name.clone(),
                        detail: "compound PRIORITY output is not a context".into(),
                    });
                };
                context
                    .get(name)
                    .ok_or_else(|| DmnError::HitPolicyViolation {
                        decision: decision.name.clone(),
                        detail: format!("compound PRIORITY output is missing '{name}'"),
                    })?
            };
            key.push(self.output_rank(decision, priorities, component, state, compiled)?);
        }
        Ok(key)
    }

    fn output_rank(
        &self,
        decision: &Decision,
        priorities: &[String],
        value: &FeelValue,
        state: &mut ExecutionState,
        compiled: &CompiledDmnModel<'_>,
    ) -> Result<usize> {
        for (rank, priority) in priorities.iter().enumerate() {
            if self.eval_unary_tests(
                compiled,
                priority,
                value,
                &mut state.context,
                &decision.name,
            )? {
                return Ok(rank);
            }
        }
        Err(DmnError::HitPolicyViolation {
            decision: decision.name.clone(),
            detail: "output is absent from outputValues".into(),
        })
    }

    fn output_order(
        &self,
        decision: &Decision,
        table: &DecisionTable,
        values: Vec<FeelValue>,
        state: &mut ExecutionState,
        compiled: &CompiledDmnModel<'_>,
    ) -> Result<FeelValue> {
        if table.outputs.len() != 1 {
            return Err(DmnError::InvalidDecisionTable {
                decision: decision.name.clone(),
                detail: "OUTPUT ORDER requires a single output".into(),
            });
        }
        let priorities = table.outputs[0].output_values.as_ref().ok_or_else(|| {
            DmnError::InvalidDecisionTable {
                decision: decision.name.clone(),
                detail: "OUTPUT ORDER requires outputValues".into(),
            }
        })?;
        let mut ranked = Vec::new();
        for value in values {
            let rank = self.output_rank(decision, priorities, &value, state, compiled)?;
            ranked.push((rank, value));
        }
        ranked.sort_by_key(|(rank, _)| *rank);
        Ok(FeelValue::List(
            ranked.into_iter().map(|(_, value)| value).collect(),
        ))
    }
    fn collect(
        &self,
        decision: &Decision,
        table: &DecisionTable,
        values: Vec<FeelValue>,
    ) -> Result<FeelValue> {
        match table.aggregation {
            None => Ok(FeelValue::List(values)),
            Some(Aggregation::Count) => Ok(FeelValue::Number(values.len() as f64)),
            Some(aggregation) => {
                if table.outputs.len() != 1 {
                    return Err(DmnError::InvalidDecisionTable {
                        decision: decision.name.clone(),
                        detail: "COLLECT aggregation requires a single output".into(),
                    });
                }
                let numbers = values
                    .into_iter()
                    .map(|value| match value {
                        FeelValue::Number(value) => Ok(value),
                        _ => Err(DmnError::HitPolicyViolation {
                            decision: decision.name.clone(),
                            detail: "COLLECT numeric aggregation requires number outputs".into(),
                        }),
                    })
                    .collect::<Result<Vec<_>>>()?;
                match aggregation {
                    Aggregation::Sum => Ok(FeelValue::Number(numbers.iter().sum())),
                    Aggregation::Min => numbers
                        .into_iter()
                        .reduce(f64::min)
                        .map(FeelValue::Number)
                        .ok_or_else(|| DmnError::HitPolicyViolation {
                            decision: decision.name.clone(),
                            detail: "MIN requires at least one matching rule".into(),
                        }),
                    Aggregation::Max => numbers
                        .into_iter()
                        .reduce(f64::max)
                        .map(FeelValue::Number)
                        .ok_or_else(|| DmnError::HitPolicyViolation {
                            decision: decision.name.clone(),
                            detail: "MAX requires at least one matching rule".into(),
                        }),
                    Aggregation::Count => unreachable!(),
                }
            }
        }
    }
}
