use super::*;
use crate::feel::FeelValue;
use std::collections::HashMap;

const SIMPLETABLE_U: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resource/0004-simpletable-U.dmn"
));
const SIMPLETABLE_F: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resource/0004-simpletable-F.dmn"
));
const SIMPLETABLE_R: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resource/0004-simpletable-R.dmn"
));
const SIMPLETABLE_O: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resource/0004-simpletable-O.dmn"
));
const SIMPLETABLE_C_SUM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resource/0004-simpletable-C-sum.dmn"
));
const SIMPLETABLE_C_MIN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resource/0004-simpletable-C-min.dmn"
));
const SIMPLETABLE_C_MAX: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resource/0004-simpletable-C-max.dmn"
));
const DEFAULT_OUTPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resource/decisiontable-default-value.dmn"
));
const INPUT_DATA_ITEM_DEFINITION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resource/0001-input-data-item-def.dmn"
));
const DECISION_SERVICE_WITH_IMPORT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resource/DSWithImportRequiredInput20180920.dmn"
));

const TYPED_ALLOWED_VALUES: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resource/typed-allowed-values.dmn"
));

const DINNER_F: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/resource/dinner.dmn"));

fn decision_table_input(
    age: f64,
    risk_category: &str,
    is_affordable: bool,
) -> HashMap<String, FeelValue> {
    HashMap::from([
        ("Age".into(), FeelValue::Number(age)),
        (
            "RiskCategory".into(),
            FeelValue::String(risk_category.into()),
        ),
        ("isAffordable".into(), FeelValue::Bool(is_affordable)),
    ])
}

fn model(table: &str) -> DmnModel {
    parse_dmn(&format!(r##"<definitions xmlns="{DMN_NAMESPACE}"><inputData id="age" name="age"><variable name="age"/></inputData><decision id="result" name="result"><informationRequirement><requiredInput href="#age"/></informationRequirement>{table}</decision></definitions>"##)).unwrap()
}

#[test]
fn executes_a_model_with_an_item_definition() {
    let model = parse_dmn(INPUT_DATA_ITEM_DEFINITION).unwrap();
    assert_eq!(model.input_data.len(), 1);
    assert_eq!(model.input_data[0].name, "Full Name");
    assert_eq!(model.decisions.len(), 1);
    assert_eq!(model.decisions[0].name, "Greeting Message");
    assert!(matches!(
        &model.decisions[0].logic,
        DecisionLogic::LiteralExpression(expression) if expression == "\"Hello \" + Full Name"
    ));
    assert_eq!(model.input_data[0].variable.as_deref(), Some("Full Name"));
}

#[test]
fn ignores_non_executable_dmn_elements_without_affecting_execution() {
    let model = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}"><import name="Other" namespace="urn:other" importType="{DMN_NAMESPACE}"/><itemDefinition id="item" name="Customer"/><knowledgeSource id="source" name="Policy"/><businessKnowledgeModel id="bkm" name="Score"><variable name="Score"/></businessKnowledgeModel><decisionService id="service" name="Service"><outputDecision href="#result"/></decisionService><textAnnotation id="note"><text>Ignored by executable parser</text></textAnnotation><authorityRequirement id="authority"><requiredAuthority href="#source"/></authorityRequirement><inputData id="age" name="age"><variable name="age"/></inputData><decision id="result" name="result"><informationRequirement><requiredInput href="#age"/></informationRequirement><decisionTable><input><inputExpression><text>age</text></inputExpression></input><output name="result"/><annotation name="note"/><rule><inputEntry><text>&gt;= 18</text></inputEntry><outputEntry><text>"adult"</text></outputEntry><annotationEntry><text>approved</text></annotationEntry></rule></decisionTable></decision></definitions>"##
    ))
    .unwrap();

    assert_eq!(model.input_data.len(), 1);
    assert_eq!(model.decisions.len(), 1);
    assert_eq!(model.decisions[0].name, "result");
    assert_eq!(
        DmnExecutor::new(&model)
            .evaluate(
                "result",
                HashMap::from([("age".into(), FeelValue::Number(20.0))])
            )
            .unwrap()
            .value,
        FeelValue::String("adult".into())
    );
}

#[test]
fn imported_decision_service_model_reports_unsupported_external_references() {
    let model = parse_dmn(DECISION_SERVICE_WITH_IMPORT).unwrap();
    assert_eq!(model.decisions.len(), 2);
    assert!(
        model
            .decisions
            .iter()
            .any(|decision| decision.name == "Decision B")
    );
    assert!(
        model
            .decisions
            .iter()
            .any(|decision| decision.name == "Invoke Decision B DS")
    );
    assert!(matches!(
        compile_dmn(&model),
        Err(DmnError::UnknownReference { reference, .. })
            if reference == "http://www.trisotech.com/dmn/definitions/_06945eb5-d665-41eb-a96a-3091b2e4e82f#_0b30a69e-76c9-433b-8e20-07bd6bef68a7"
    ));
}
#[test]
fn parses_default_namespace_and_executes_unique_table() {
    let model = model(
        r#"<decisionTable hitPolicy="UNIQUE"><input><inputExpression><text>age</text></inputExpression></input><output name="result"/><rule id="adult"><inputEntry><text>>= 18</text></inputEntry><outputEntry><text>"adult"</text></outputEntry></rule></decisionTable>"#,
    );
    let result = DmnExecutor::new(&model)
        .evaluate(
            "result",
            HashMap::from([("age".into(), FeelValue::Number(20.0))]),
        )
        .unwrap();
    assert_eq!(result.value, FeelValue::String("adult".into()));
    assert_eq!(result.matched_rule_ids, vec!["adult"]);
}

#[test]
fn anonymous_rules_receive_stable_trace_ids() {
    let model = model(
        r#"<decisionTable hitPolicy="UNIQUE"><input><inputExpression><text>age</text></inputExpression></input><output name="result"/><rule><inputEntry><text>>= 18</text></inputEntry><outputEntry><text>"adult"</text></outputEntry></rule></decisionTable>"#,
    );
    let result = DmnExecutor::new(&model)
        .evaluate(
            "result",
            HashMap::from([("age".into(), FeelValue::Number(20.0))]),
        )
        .unwrap();
    assert_eq!(result.matched_rule_ids, vec!["rule-1"]);
}
#[test]
fn evaluates_literal_dependencies() {
    let model = parse_dmn(&format!(r##"<x:definitions xmlns:x="{DMN_NAMESPACE}"><x:inputData id="a" name="a"><x:variable name="a"/></x:inputData><x:decision id="b" name="b"><x:informationRequirement><x:requiredInput href="#a"/></x:informationRequirement><x:literalExpression><x:text>a + 1</x:text></x:literalExpression></x:decision><x:decision id="c" name="c"><x:informationRequirement><x:requiredDecision href="#b"/></x:informationRequirement><x:literalExpression><x:text>b * 2</x:text></x:literalExpression></x:decision></x:definitions>"##)).unwrap();
    let result = DmnExecutor::new(&model)
        .evaluate("c", HashMap::from([("a".into(), FeelValue::Number(3.0))]))
        .unwrap();
    assert_eq!(result.value, FeelValue::Number(8.0));
}
#[test]
fn collects_and_aggregates() {
    let model = model(
        r#"<decisionTable hitPolicy="COLLECT" aggregation="SUM"><input><inputExpression><text>age</text></inputExpression></input><output name="result"/><rule><inputEntry><text>>= 18</text></inputEntry><outputEntry><text>2</text></outputEntry></rule><rule><inputEntry><text>>= 10</text></inputEntry><outputEntry><text>3</text></outputEntry></rule></decisionTable>"#,
    );
    let result = DmnExecutor::new(&model)
        .evaluate(
            "result",
            HashMap::from([("age".into(), FeelValue::Number(20.0))]),
        )
        .unwrap();
    assert_eq!(result.value, FeelValue::Number(5.0));
}

#[test]
fn applies_first_any_rule_and_output_order_policies() {
    let input = HashMap::from([("age".into(), FeelValue::Number(20.0))]);
    let first = model(
        r#"<decisionTable hitPolicy="FIRST"><input><inputExpression><text>age</text></inputExpression></input><output name="result"/><rule id="first"><inputEntry><text>>= 18</text></inputEntry><outputEntry><text>"first"</text></outputEntry></rule><rule id="second"><inputEntry><text>>= 10</text></inputEntry><outputEntry><text>"second"</text></outputEntry></rule></decisionTable>"#,
    );
    assert_eq!(
        DmnExecutor::new(&first)
            .evaluate("result", input.clone())
            .unwrap()
            .value,
        FeelValue::String("first".into())
    );

    let any = model(
        r#"<decisionTable hitPolicy="ANY"><input><inputExpression><text>age</text></inputExpression></input><output name="result"/><rule><inputEntry><text>>= 18</text></inputEntry><outputEntry><text>"same"</text></outputEntry></rule><rule><inputEntry><text>>= 10</text></inputEntry><outputEntry><text>"same"</text></outputEntry></rule></decisionTable>"#,
    );
    assert_eq!(
        DmnExecutor::new(&any)
            .evaluate("result", input.clone())
            .unwrap()
            .value,
        FeelValue::String("same".into())
    );

    let rule_order = model(
        r#"<decisionTable hitPolicy="RULE ORDER"><input><inputExpression><text>age</text></inputExpression></input><output name="result"/><rule><inputEntry><text>>= 18</text></inputEntry><outputEntry><text>2</text></outputEntry></rule><rule><inputEntry><text>>= 10</text></inputEntry><outputEntry><text>1</text></outputEntry></rule></decisionTable>"#,
    );
    assert_eq!(
        DmnExecutor::new(&rule_order)
            .evaluate("result", input.clone())
            .unwrap()
            .value,
        FeelValue::List(vec![FeelValue::Number(2.0), FeelValue::Number(1.0)])
    );

    let output_order = model(
        r#"<decisionTable hitPolicy="OUTPUT ORDER"><input><inputExpression><text>age</text></inputExpression></input><output name="result"><outputValues><text>"high", "low"</text></outputValues></output><rule><inputEntry><text>>= 18</text></inputEntry><outputEntry><text>"low"</text></outputEntry></rule><rule><inputEntry><text>>= 10</text></inputEntry><outputEntry><text>"high"</text></outputEntry></rule></decisionTable>"#,
    );
    assert_eq!(
        DmnExecutor::new(&output_order)
            .evaluate("result", input)
            .unwrap()
            .value,
        FeelValue::List(vec![
            FeelValue::String("high".into()),
            FeelValue::String("low".into())
        ])
    );
}

#[test]
fn applies_priority_order_policies() {
    let priority_order = parse_dmn(
        &r##"<?xml version="1.0" encoding="UTF-8"?>
            <definitions xmlns="https://www.omg.org/spec/DMN/20230324/MODEL/"
                         xmlns:feel="https://www.omg.org/spec/DMN/20230324/FEEL/"
                         id="riskAssessment"
                         name="风险评估"
                         namespace="http://example.com/dmn/risk">
              <!-- =====================================================
                   1. 定义类型：用 allowedValues 约束输出值 + 定义优先级顺序
                   列表中的顺序就是优先级：索引 0 优先级最高
                   ===================================================== -->
              <itemDefinition id="riskLevelType" name="RiskLevel">
                <allowedValues>
                  <text>"高", "中", "低"</text>
                </allowedValues>
              </itemDefinition>

              <itemDefinition id="scoreType" name="Score">
                <typeRef>feel:number</typeRef>
              </itemDefinition>

              <itemDefinition id="descType" name="Description">
                <typeRef>feel:string</typeRef>
              </itemDefinition>

              <!-- =====================================================
                   2. 决策表：hitPolicy="PRIORITY"
                   ===================================================== -->
              <decision id="assessRisk" name="风险评估">
                <variable name="riskResult"/>

                <decisionTable id="dt_risk" hitPolicy="PRIORITY" preferredOrientation="Rule-as-Row">

                  <!-- 输入 -->
                  <input id="in_age" label="年龄">
                    <inputExpression typeRef="feel:number" id="expr_age">
                      <text>age</text>
                    </inputExpression>
                    <inputValues>
                      <text>[0..200]</text>
                    </inputValues>
                  </input>

                  <input id="in_health" label="健康指标(0-100)">
                    <inputExpression typeRef="feel:number" id="expr_health">
                      <text>healthScore</text>
                    </inputExpression>
                    <inputValues>
                      <text>[0..100]</text>
                    </inputValues>
                  </input>

                  <!-- 输出：引用上面定义的 riskLevelType -->
                  <output id="out_level" name="风险等级" label="风险等级" typeRef="RiskLevel">
                    <outputValues><text>"高", "中", "低"</text></outputValues>
                  </output>
                  <output id="out_score" name="风险分" label="风险分" typeRef="Score"/>
                  <output id="out_desc" name="建议" label="建议" typeRef="Description"/>

                  <!-- =====================================================
                       3. 规则
                       允许多条命中，PRIORITY 按 allowedValues 顺序取最高优先级
                       "高"(索引0) > "中"(索引1) > "低"(索引2)
                       ===================================================== -->

                  <!-- R1: 老年 + 健康差 → 高风险 -->
                  <rule id="R1">
                    <inputEntry id="r1_age">
                      <text>>= 60</text>
                    </inputEntry>
                    <inputEntry id="r1_health">
                      <text>&lt; 40</text>
                    </inputEntry>
                    <outputEntry id="r1_level">
                      <text>"高"</text>
                    </outputEntry>
                    <outputEntry id="r1_score">
                      <text>85</text>
                    </outputEntry>
                    <outputEntry id="r1_desc">
                      <text>"建议立即就医，安排专项检查"</text>
                    </outputEntry>
                  </rule>

                  <!-- R2: 青年 + 健康差 → 中风险 -->
                  <rule id="R2">
                    <inputEntry id="r2_age">
                      <text>&lt; 30</text>
                    </inputEntry>
                    <inputEntry id="r2_health">
                      <text>&lt; 40</text>
                    </inputEntry>
                    <outputEntry id="r2_level">
                      <text>"中"</text>
                    </outputEntry>
                    <outputEntry id="r2_score">
                      <text>50</text>
                    </outputEntry>
                    <outputEntry id="r2_desc">
                      <text>"建议改善作息，定期体检"</text>
                    </outputEntry>
                  </rule>

                  <!-- R3: 老年 → 中风险（无论健康指标） -->
                  <rule id="R3">
                    <inputEntry id="r3_age">
                      <text>>= 60</text>
                    </inputEntry>
                    <inputEntry id="r3_health">
                      <text>not(&lt; 40)</text>
                    </inputEntry>
                    <outputEntry id="r3_level">
                      <text>"中"</text>
                    </outputEntry>
                    <outputEntry id="r3_score">
                      <text>60</text>
                    </outputEntry>
                    <outputEntry id="r3_desc">
                      <text>"建议每年全面体检，关注慢性病"</text>
                    </outputEntry>
                  </rule>

                  <!-- R4: 中年 + 健康一般 → 低风险 -->
                  <rule id="R4">
                    <inputEntry id="r4_age">
                      <text>[30..59]</text>
                    </inputEntry>
                    <inputEntry id="r4_health">
                      <text>[40..70)</text>
                    </inputEntry>
                    <outputEntry id="r4_level">
                      <text>"低"</text>
                    </outputEntry>
                    <outputEntry id="r4_score">
                      <text>25</text>
                    </outputEntry>
                    <outputEntry id="r4_desc">
                      <text>"保持良好生活习惯即可"</text>
                    </outputEntry>
                  </rule>

                  <!-- R5: 青年 + 健康好 → 低风险 -->
                  <rule id="R5">
                    <inputEntry id="r5_age">
                      <text>&lt; 30</text>
                    </inputEntry>
                    <inputEntry id="r5_health">
                      <text>>= 40</text>
                    </inputEntry>
                    <outputEntry id="r5_level">
                      <text>"低"</text>
                    </outputEntry>
                    <outputEntry id="r5_score">
                      <text>15</text>
                    </outputEntry>
                    <outputEntry id="r5_desc">
                      <text>"身体状况良好，继续保持"</text>
                    </outputEntry>
                  </rule>

                  <!-- R6: 任意年龄 + 健康优秀 → 低风险（兜底宽规则） -->
                  <rule id="R6">
                    <inputEntry id="r6_age">
                      <text>not(&lt; 30)</text>
                    </inputEntry>
                    <inputEntry id="r6_health">
                      <text>>= 70</text>
                    </inputEntry>
                    <outputEntry id="r6_level">
                      <text>"低"</text>
                    </outputEntry>
                    <outputEntry id="r6_score">
                      <text>10</text>
                    </outputEntry>
                    <outputEntry id="r6_desc">
                      <text>"健康状况优秀"</text>
                    </outputEntry>
                  </rule>

                </decisionTable>
              </decision>
            </definitions>
            "##
        .to_string(),
    )
    .unwrap();
    //age=65, healthScore=90
    let input = HashMap::from([
        ("age".into(), FeelValue::Number(65.0)),
        ("healthScore".into(), FeelValue::Number(90.0)),
    ]);

    assert_eq!(
        DmnExecutor::new(&priority_order)
            .evaluate("assessRisk", input)
            .unwrap()
            .value,
        FeelValue::Context(HashMap::from([
            ("风险等级".into(), FeelValue::String("中".into())),
            ("风险分".into(), FeelValue::Number(60.0)),
            (
                "建议".into(),
                FeelValue::String("建议每年全面体检，关注慢性病".into())
            ),
        ]))
    );
}

#[test]
fn returns_default_output_and_propagates_default_expression_errors() {
    let with_default = model(
        r#"<decisionTable hitPolicy="UNIQUE"><input><inputExpression><text>age</text></inputExpression></input><output name="result" defaultOutputEntry="&quot;minor&quot;"/><rule><inputEntry><text>>= 18</text></inputEntry><outputEntry><text>"adult"</text></outputEntry></rule></decisionTable>"#,
    );
    assert_eq!(
        DmnExecutor::new(&with_default)
            .evaluate(
                "result",
                HashMap::from([("age".into(), FeelValue::Number(10.0))])
            )
            .unwrap()
            .value,
        FeelValue::String("minor".into())
    );

    let invalid_default = model(
        r#"<decisionTable hitPolicy="UNIQUE"><input><inputExpression><text>age</text></inputExpression></input><output name="result" defaultOutputEntry="missing"/><rule><inputEntry><text>>= 18</text></inputEntry><outputEntry><text>"adult"</text></outputEntry></rule></decisionTable>"#,
    );
    assert!(matches!(
        DmnExecutor::new(&invalid_default).evaluate(
            "result",
            HashMap::from([("age".into(), FeelValue::Number(10.0))])
        ),
        Err(DmnError::Feel { .. })
    ));
}

#[test]
fn rejects_any_and_unique_policy_conflicts() {
    for policy in ["ANY", "UNIQUE"] {
        let model = model(&format!(
            r#"<decisionTable hitPolicy="{policy}"><input><inputExpression><text>age</text></inputExpression></input><output name="result"/><rule><inputEntry><text>>= 18</text></inputEntry><outputEntry><text>"a"</text></outputEntry></rule><rule><inputEntry><text>>= 10</text></inputEntry><outputEntry><text>"b"</text></outputEntry></rule></decisionTable>"#
        ));
        assert!(matches!(
            DmnExecutor::new(&model).evaluate(
                "result",
                HashMap::from([("age".into(), FeelValue::Number(20.0))])
            ),
            Err(DmnError::HitPolicyViolation { .. })
        ));
    }
}

#[test]
fn compile_rejects_unknown_references_and_cycles() {
    let unknown = parse_dmn(&format!(r##"<definitions xmlns="{DMN_NAMESPACE}"><decision id="a" name="a"><informationRequirement><requiredDecision href="#missing"/></informationRequirement><literalExpression><text>1</text></literalExpression></decision></definitions>"##)).unwrap();
    assert!(matches!(
        compile_dmn(&unknown),
        Err(DmnError::UnknownReference { .. })
    ));

    let cycle = parse_dmn(&format!(r##"<definitions xmlns="{DMN_NAMESPACE}"><decision id="a" name="a"><informationRequirement><requiredDecision href="#b"/></informationRequirement><literalExpression><text>1</text></literalExpression></decision><decision id="b" name="b"><informationRequirement><requiredDecision href="#a"/></informationRequirement><literalExpression><text>1</text></literalExpression></decision></definitions>"##)).unwrap();
    assert!(matches!(
        compile_dmn(&cycle),
        Err(DmnError::CircularDependency(_))
    ));
}

#[test]
fn result_api_collects_failures_and_runs_independent_decisions() {
    let model = parse_dmn(&format!(r##"<definitions xmlns="{DMN_NAMESPACE}"><inputData id="age" name="age"><variable name="age"/></inputData><decision id="broken" name="broken"><informationRequirement><requiredInput href="#age"/></informationRequirement><literalExpression><text>substring("x", 0)</text></literalExpression></decision><decision id="independent" name="independent"><literalExpression><text>42</text></literalExpression></decision><decision id="dependent" name="dependent"><informationRequirement><requiredDecision href="#broken"/></informationRequirement><literalExpression><text>broken + 1</text></literalExpression></decision></definitions>"##)).unwrap();
    let result = DmnExecutor::new(&model)
        .evaluate_all(HashMap::from([("age".into(), FeelValue::Number(1.0))]));
    assert!(result.has_errors());
    assert_eq!(result.status, DmnResultStatus::Failed);
    assert_eq!(
        result.decision_by_id("broken").unwrap().status,
        DecisionStatus::Failed
    );
    assert_eq!(
        result.decision_by_id("dependent").unwrap().status,
        DecisionStatus::Skipped
    );
    assert_eq!(
        result
            .decision_by_id("dependent")
            .unwrap()
            .evaluated_dependencies,
        vec!["broken"]
    );
    assert_eq!(
        result.decision_by_id("independent").unwrap().value,
        FeelValue::Number(42.0)
    );
    assert!(result.decision_by_name("independent").is_some());
    assert!(result.messages.iter().any(|message| {
        message.code == "DMN-EVAL-FEEL" && message.source.decision_id.as_deref() == Some("broken")
    }));
    assert!(result.messages.iter().any(|message| {
        message.code == "DMN-EVAL-DEPENDENCY"
            && message.source.decision_id.as_deref() == Some("dependent")
    }));
}

#[test]
fn parse_dmn_rejects_invalid_roots_and_required_attributes() {
    for xml in ["<definitions/>", "<definitions xmlns=\"urn:not-dmn\"/>"] {
        assert!(
            matches!(parse_dmn(xml), Err(DmnError::Xml(detail)) if detail.contains("supported DMN definitions"))
        );
    }
    let missing_id =
        format!(r#"<definitions xmlns="{DMN_NAMESPACE}"><inputData name="age"/></definitions>"#);
    assert!(matches!(
        parse_dmn(&missing_id),
        Err(DmnError::MissingAttribute { element, attribute }) if element == "inputData" && attribute == "id"
    ));
    let missing_href = format!(
        r#"<definitions xmlns="{DMN_NAMESPACE}"><decision id="d" name="d"><informationRequirement><requiredInput/></informationRequirement><literalExpression><text>1</text></literalExpression></decision></definitions>"#
    );
    assert!(matches!(
        parse_dmn(&missing_href),
        Err(DmnError::MissingAttribute { element, attribute }) if element == "requiredInput" && attribute == "href"
    ));
}

#[test]
fn parses_dmn_20231201_namespace_with_prefixed_definitions() {
    let xml = format!(
        r##"<dmn:definitions xmlns:dmn="{DMN_20231201_NAMESPACE}"><dmn:inputData id="age-input" name="ApplicantAge"><dmn:variable name="ApplicantAge" typeRef="feel:number"/></dmn:inputData><dmn:decision id="adult" name="Adult"><dmn:informationRequirement><dmn:requiredInput href="#age-input"/></dmn:informationRequirement><dmn:literalExpression><dmn:text>ApplicantAge &gt;= 18</dmn:text></dmn:literalExpression></dmn:decision></dmn:definitions>"##
    );
    let model = parse_dmn(&xml).expect("DMN 2023-12-01 should be supported");
    assert_eq!(
        model.input_data[0].variable.as_deref(),
        Some("ApplicantAge")
    );
    assert!(compile_dmn(&model).is_ok());
}

#[test]
fn parse_dmn_ignores_foreign_namespace_elements_and_decodes_cdata() {
    let xml = format!(
        r#"<definitions xmlns="{DMN_NAMESPACE}" xmlns:ext="urn:extension"><ext:decision id="foreign" name="foreign"/><decision id="valid" name="valid"><literalExpression><![CDATA[40 + 2]]></literalExpression></decision></definitions>"#
    );
    let model = parse_dmn(&xml).unwrap();
    assert_eq!(model.decisions.len(), 1);
    assert_eq!(model.decisions[0].id, "valid");
    assert_eq!(
        DmnExecutor::new(&model)
            .evaluate("valid", HashMap::new())
            .unwrap()
            .value,
        FeelValue::Number(42.0)
    );
}

#[test]
fn compile_dmn_validates_duplicate_names_feel_and_table_shape() {
    let duplicate_names = parse_dmn(&format!(
        r#"<definitions xmlns="{DMN_NAMESPACE}"><decision id="a" name="same"><literalExpression><text>1</text></literalExpression></decision><decision id="b" name="same"><literalExpression><text>2</text></literalExpression></decision></definitions>"#
    )).unwrap();
    assert!(
        matches!(compile_dmn(&duplicate_names), Err(DmnError::Xml(detail)) if detail.contains("duplicate decision name 'same'"))
    );

    let invalid_feel = parse_dmn(&format!(
        r#"<definitions xmlns="{DMN_NAMESPACE}"><decision id="bad" name="bad"><literalExpression><text>1 +</text></literalExpression></decision></definitions>"#
    )).unwrap();
    assert!(
        matches!(compile_dmn(&invalid_feel), Err(DmnError::Feel { decision, .. }) if decision == "bad")
    );

    let invalid_shape = parse_dmn(&format!(
        r#"<definitions xmlns="{DMN_NAMESPACE}"><decision id="bad_table" name="bad_table"><decisionTable><input><inputExpression><text>1</text></inputExpression></input><output name="result"/><rule><inputEntry><text>-</text></inputEntry></rule></decisionTable></decision></definitions>"#
    )).unwrap();
    assert!(
        matches!(compile_dmn(&invalid_shape), Err(DmnError::InvalidDecisionTable { detail, .. }) if detail.contains("rule entry count"))
    );
}

#[test]
fn compiled_executor_and_target_selection_preserve_public_api() {
    let model = parse_dmn(&format!(
        r#"<definitions xmlns="{DMN_NAMESPACE}"><decision id="decision-id" name="Decision Name"><literalExpression><text>42</text></literalExpression></decision></definitions>"#
    )).unwrap();
    let compiled = compile_dmn(&model).unwrap();
    let executor = DmnExecutor::from_compiled(&compiled);
    assert_eq!(
        executor
            .evaluate("decision-id", HashMap::new())
            .unwrap()
            .value,
        FeelValue::Number(42.0)
    );
    assert_eq!(
        executor
            .evaluate_by_name("Decision Name", HashMap::new())
            .decision_by_id("decision-id")
            .unwrap()
            .value,
        FeelValue::Number(42.0)
    );
    let missing = executor.evaluate_by_id("missing", HashMap::new());
    assert_eq!(missing.status, DmnResultStatus::Failed);
    assert!(missing.decision_results.is_empty());
    assert_eq!(missing.messages[0].code, "DMN-EVAL-ERROR");
    assert!(matches!(
        executor.evaluate("missing", HashMap::new()),
        Err(DmnError::UnknownDecision(decision)) if decision == "missing"
    ));
}

#[test]
fn evaluate_missing_required_input_returns_structured_and_fail_fast_errors() {
    let model = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}"><inputData id="age" name="age"><variable name="age"/></inputData><decision id="required" name="required"><informationRequirement><requiredInput href="#age"/></informationRequirement><literalExpression><text>age + 1</text></literalExpression></decision></definitions>"##
    )).unwrap();
    let result = DmnExecutor::new(&model).evaluate_by_id("required", HashMap::new());
    let decision = result.decision_by_id("required").unwrap();
    assert_eq!(decision.status, DecisionStatus::Skipped);
    assert_eq!(decision.value, FeelValue::Null);
    assert!(decision.matched_rule_ids.is_empty());
    assert!(result.messages.iter().any(|message| {
        message.code == "DMN-EVAL-MISSING-INPUT"
            && message.source.decision_id.as_deref() == Some("required")
    }));
    assert!(matches!(
        DmnExecutor::new(&model).evaluate("required", HashMap::new()),
        Err(DmnError::MissingInput(input)) if input == "age"
    ));
}

#[test]
fn fixture_decision_tables_cover_legacy_namespaces_and_hit_policies() {
    let unique = parse_dmn(SIMPLETABLE_U).unwrap();
    assert_eq!(unique.input_data.len(), 3);
    let approved = DmnExecutor::new(&unique)
        .evaluate(
            "_0004-simpletable-U",
            decision_table_input(60.0, "Low", true),
        )
        .unwrap();
    assert_eq!(approved.value, FeelValue::String("Approved".into()));
    assert_eq!(
        approved.matched_rule_ids,
        vec!["_7f03803d-2636-40ab-8346-7fd7f38ab695"]
    );
    assert_eq!(
        DmnExecutor::new(&unique)
            .evaluate(
                "_0004-simpletable-U",
                decision_table_input(60.0, "High", true),
            )
            .unwrap()
            .value,
        FeelValue::String("Declined".into())
    );

    let first = DmnExecutor::new(&parse_dmn(SIMPLETABLE_F).unwrap())
        .evaluate(
            "_0004-simpletable-F",
            decision_table_input(60.0, "Low", true),
        )
        .unwrap();
    assert_eq!(
        first.value,
        FeelValue::Context(HashMap::from([
            (
                "Approval Status".into(),
                FeelValue::String("Approved".into())
            ),
            (
                "Decision Review".into(),
                FeelValue::String("Decision final".into()),
            ),
        ]))
    );
    assert_eq!(
        first.matched_rule_ids,
        vec!["_7f03803d-2636-40ab-8346-7fd7f38ab695"]
    );

    let rule_order = DmnExecutor::new(&parse_dmn(SIMPLETABLE_R).unwrap())
        .evaluate(
            "_0004-simpletable-R",
            decision_table_input(60.0, "Low", true),
        )
        .unwrap();
    assert_eq!(
        rule_order.value,
        FeelValue::List(vec![
            FeelValue::String("Approved".into()),
            FeelValue::String("Needs review".into()),
            FeelValue::String("Declined".into()),
        ])
    );

    let output_order = DmnExecutor::new(&parse_dmn(SIMPLETABLE_O).unwrap())
        .evaluate(
            "_0004-simpletable-O",
            decision_table_input(60.0, "Low", true),
        )
        .unwrap();
    assert_eq!(
        output_order.value,
        FeelValue::List(vec![
            FeelValue::String("Declined".into()),
            FeelValue::String("Declined".into()),
            FeelValue::String("Approved".into()),
        ])
    );
}

#[test]
fn fixture_collect_aggregations_and_nested_default_output_execute() {
    for (fixture, decision, expected) in [
        (SIMPLETABLE_C_SUM, "_0004-simpletable-C-sum", 48.0),
        (SIMPLETABLE_C_MIN, "_0004-simpletable-C-min", 10.0),
        (SIMPLETABLE_C_MAX, "_0004-simpletable-C-max", 25.0),
    ] {
        let result = DmnExecutor::new(&parse_dmn(fixture).unwrap())
            .evaluate(decision, decision_table_input(60.0, "Low", true))
            .unwrap();
        assert_eq!(result.value, FeelValue::Number(expected));
        assert_eq!(result.matched_rule_ids.len(), 3);
    }

    let default_output = DmnExecutor::new(&parse_dmn(DEFAULT_OUTPUT).unwrap())
        .evaluate(
            "d_decisiontable_default_value",
            decision_table_input(10.0, "Low", true),
        )
        .unwrap();
    assert_eq!(default_output.value, FeelValue::String("Declined".into()));
    assert!(default_output.matched_rule_ids.is_empty());
}

#[test]
fn serializes_dmn_models_and_persistent_artifacts() {
    let original = model(
        r#"<decisionTable><input><inputExpression><text>age</text></inputExpression></input><output name="result"/><rule><inputEntry><text>>= 18</text></inputEntry><outputEntry><text>"adult"</text></outputEntry></rule></decisionTable>"#,
    );
    let xml = original.to_dmn_xml().unwrap();
    let restored = parse_dmn(&xml).unwrap();
    assert_eq!(
        DmnExecutor::new(&restored)
            .evaluate(
                "result",
                HashMap::from([("age".into(), FeelValue::Number(20.0))])
            )
            .unwrap()
            .value,
        FeelValue::String("adult".into())
    );

    let compiled = compile_owned(restored).unwrap();
    let artifact = compiled.to_artifact_json().unwrap();
    let loaded = OwnedCompiledDmnModel::from_artifact_json(&artifact).unwrap();
    assert_eq!(
        loaded
            .evaluate_by_id(
                "result",
                HashMap::from([("age".into(), FeelValue::Number(20.0))])
            )
            .decision_by_id("result")
            .unwrap()
            .value,
        FeelValue::String("adult".into())
    );
    assert!(matches!(
        OwnedCompiledDmnModel::from_artifact_json(r#"{"format_version":999,"model":{}}"#),
        Err(DmnError::Artifact(_))
    ));

    let typed = parse_dmn(TYPED_ALLOWED_VALUES).unwrap();
    let typed = parse_dmn(&typed.to_dmn_xml().unwrap()).unwrap();
    assert_eq!(typed.item_definitions.len(), 1);
    assert_eq!(
        typed.item_definitions[0].allowed_values.as_deref(),
        Some("\"EMPLOYED\", \"UNEMPLOYED\"")
    );
    assert_eq!(
        typed.input_data[0].type_ref.as_deref(),
        Some("EmploymentStatus")
    );
    assert_eq!(typed.decisions[0].type_ref.as_deref(), Some("string"));
}

#[test]
fn context_entries_are_scoped_and_evaluate_in_order() {
    let model = parse_dmn(&format!(
        r#"<definitions xmlns="{DMN_NAMESPACE}"><decision id="result" name="result"><context><contextEntry><variable name="base"/><literalExpression><text>40</text></literalExpression></contextEntry><contextEntry><variable name="answer"/><literalExpression><text>base + 2</text></literalExpression></contextEntry></context></decision><decision id="independent" name="independent"><literalExpression><text>base</text></literalExpression></decision></definitions>"#
    )).unwrap();
    let result = DmnExecutor::new(&model)
        .evaluate("result", HashMap::new())
        .unwrap();
    assert_eq!(
        result.value,
        FeelValue::Context(HashMap::from([
            ("base".into(), FeelValue::Number(40.0)),
            ("answer".into(), FeelValue::Number(42.0)),
        ]))
    );
    assert!(matches!(
        DmnExecutor::new(&model).evaluate("independent", HashMap::new()),
        Err(DmnError::Feel { .. })
    ));
}

#[test]
fn context_bindings_are_restored_after_evaluation_errors() {
    let model = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}">
            <decision id="failing" name="failing">
                <context>
                    <contextEntry><variable name="temporary"/><literalExpression><text>42</text></literalExpression></contextEntry>
                    <contextEntry><literalExpression><text>missing value</text></literalExpression></contextEntry>
                </context>
            </decision>
            <decision id="independent" name="independent"><literalExpression><text>temporary</text></literalExpression></decision>
        </definitions>"##
    ))
    .unwrap();
    let result = DmnExecutor::new(&model).evaluate_all(HashMap::from([(
        "temporary".into(),
        FeelValue::Number(7.0),
    )]));
    assert_eq!(
        result.decision_by_id("independent").unwrap().value,
        FeelValue::Number(7.0)
    );
    assert_eq!(
        result.decision_by_id("failing").unwrap().status,
        DecisionStatus::Failed
    );
}

#[test]
fn decision_table_supports_multi_output_and_collect_edge_cases() {
    let multi_output = model(
        r#"<decisionTable><input><inputExpression><text>age</text></inputExpression></input><output name="risk"/><output name="limit"/><rule id="approved"><inputEntry><text>>= 18</text></inputEntry><outputEntry><text>"high"</text></outputEntry><outputEntry><text>100</text></outputEntry></rule></decisionTable>"#,
    );
    let result = DmnExecutor::new(&multi_output)
        .evaluate(
            "result",
            HashMap::from([("age".into(), FeelValue::Number(20.0))]),
        )
        .unwrap();
    assert_eq!(
        result.value,
        FeelValue::Context(HashMap::from([
            ("risk".into(), FeelValue::String("high".into())),
            ("limit".into(), FeelValue::Number(100.0)),
        ]))
    );
    assert_eq!(result.matched_rule_ids, vec!["approved"]);

    let collect_count = model(
        r#"<decisionTable hitPolicy="COLLECT" aggregation="COUNT"><input><inputExpression><text>age</text></inputExpression></input><output name="result"/><rule><inputEntry><text>>= 18</text></inputEntry><outputEntry><text>"a"</text></outputEntry></rule><rule><inputEntry><text>>= 10</text></inputEntry><outputEntry><text>"b"</text></outputEntry></rule></decisionTable>"#,
    );
    assert_eq!(
        DmnExecutor::new(&collect_count)
            .evaluate(
                "result",
                HashMap::from([("age".into(), FeelValue::Number(20.0))])
            )
            .unwrap()
            .value,
        FeelValue::Number(2.0)
    );

    let invalid_output_order = model(
        r#"<decisionTable hitPolicy="OUTPUT ORDER"><input><inputExpression><text>age</text></inputExpression></input><output name="result"/><rule><inputEntry><text>-</text></inputEntry><outputEntry><text>1</text></outputEntry></rule></decisionTable>"#,
    );
    assert!(matches!(
        DmnExecutor::new(&invalid_output_order)
            .evaluate("result", HashMap::from([("age".into(), FeelValue::Number(20.0))])),
        Err(DmnError::InvalidDecisionTable { detail, .. }) if detail.contains("outputValues")
    ));
}

#[test]
fn executes_context_list_and_bkm_expressions() {
    let model = parse_dmn(&format!(
        r##"
        <definitions xmlns="{DMN_NAMESPACE}">
            <businessKnowledgeModel id="increment" name="increment">
                <variable name="increment" />
                <functionDefinition>
                    <formalParameter name="x" />
                    <literalExpression>
                        <text>x + 1</text>
                    </literalExpression>
                </functionDefinition>
            </businessKnowledgeModel>
            <decision id="context" name="context">
                <context>
                    <contextEntry>
                        <variable name="base" />
                        <literalExpression>
                            <text>40</text>
                        </literalExpression>
                    </contextEntry>
                    <contextEntry>
                        <variable name="answer" />
                        <literalExpression>
                            <text>base + 2</text>
                        </literalExpression>
                    </contextEntry>
                </context>
            </decision>
            <decision id="list" name="list">
                <list>
                    <literalExpression>
                        <text>1</text>
                    </literalExpression>
                    <literalExpression>
                        <text>2</text>
                    </literalExpression>
                </list>
            </decision>
            <decision id="bkm" name="bkm">
                <invocation>
                    <literalExpression>
                        <text>increment</text>
                    </literalExpression>
                    <binding>
                        <parameter name="x" />
                        <literalExpression>
                            <text>41</text>
                        </literalExpression>
                    </binding>
                </invocation>
            </decision>
        </definitions>
    "##
    ))
    .unwrap();
    let executor = DmnExecutor::new(&model);
    assert_eq!(
        executor.evaluate("context", HashMap::new()).unwrap().value,
        FeelValue::Context(HashMap::from([
            ("base".into(), FeelValue::Number(40.0)),
            ("answer".into(), FeelValue::Number(42.0))
        ]))
    );
    assert_eq!(
        executor.evaluate("list", HashMap::new()).unwrap().value,
        FeelValue::List(vec![FeelValue::Number(1.0), FeelValue::Number(2.0)])
    );
    assert_eq!(
        executor.evaluate("bkm", HashMap::new()).unwrap().value,
        FeelValue::Number(42.0)
    );
}

#[test]
fn first_short_circuits_and_multi_output_defaults_are_contexts() {
    let first = model(
        r#"<decisionTable hitPolicy="FIRST"><input><inputExpression><text>age</text></inputExpression></input><output name="result"/><rule id="first"><inputEntry><text>&gt;= 18</text></inputEntry><outputEntry><text>"first"</text></outputEntry></rule><rule id="invalid"><inputEntry><text>-</text></inputEntry><outputEntry><text>missing</text></outputEntry></rule></decisionTable>"#,
    );
    let result = DmnExecutor::new(&first)
        .evaluate(
            "result",
            HashMap::from([("age".into(), FeelValue::Number(20.0))]),
        )
        .unwrap();
    assert_eq!(result.value, FeelValue::String("first".into()));
    assert_eq!(result.matched_rule_ids, vec!["first"]);

    let defaults = model(
        r#"<decisionTable><input><inputExpression><text>age</text></inputExpression></input><output name="risk" defaultOutputEntry="&quot;low&quot;"/><output name="limit" defaultOutputEntry="age * 10"/><rule><inputEntry><text>&gt;= 18</text></inputEntry><outputEntry><text>"high"</text></outputEntry><outputEntry><text>100</text></outputEntry></rule></decisionTable>"#,
    );
    assert_eq!(
        DmnExecutor::new(&defaults)
            .evaluate(
                "result",
                HashMap::from([("age".into(), FeelValue::Number(10.0))])
            )
            .unwrap()
            .value,
        FeelValue::Context(HashMap::from([
            ("risk".into(), FeelValue::String("low".into())),
            ("limit".into(), FeelValue::Number(100.0))
        ]))
    );
}

#[test]
fn compilation_rejects_invalid_table_policy_combinations() {
    let invalid = model(
        r#"<decisionTable hitPolicy="UNIQUE" aggregation="SUM"><input><inputExpression><text>age</text></inputExpression></input><output name="result"/><rule><inputEntry><text>-</text></inputEntry><outputEntry><text>1</text></outputEntry></rule></decisionTable>"#,
    );
    assert!(matches!(
        compile_dmn(&invalid),
        Err(DmnError::InvalidDecisionTable { detail, .. }) if detail.contains("aggregation requires COLLECT")
    ));
}

#[test]
fn dmn15_priority_uses_output_values_and_preserves_commas_in_strings() {
    let model = model(
        r#"<decisionTable hitPolicy="PRIORITY"><input><inputExpression><text>age</text></inputExpression></input><output name="result"><outputValues><text>"high, urgent", "medium", "low"</text></outputValues></output><rule id="low"><inputEntry><text>>= 10</text></inputEntry><outputEntry><text>"low"</text></outputEntry></rule><rule id="high"><inputEntry><text>>= 18</text></inputEntry><outputEntry><text>"high, urgent"</text></outputEntry></rule></decisionTable>"#,
    );
    let result = DmnExecutor::new(&model)
        .evaluate(
            "result",
            HashMap::from([("age".into(), FeelValue::Number(20.0))]),
        )
        .unwrap();
    assert_eq!(result.value, FeelValue::String("high, urgent".into()));
    assert_eq!(result.matched_rule_ids, vec!["low", "high"]);
}

#[test]
fn dmn15_priority_orders_compound_outputs_left_to_right() {
    let model = model(
        r#"<decisionTable hitPolicy="PRIORITY"><input><inputExpression><text>age</text></inputExpression></input><output name="risk"><outputValues><text>"high", "low"</text></outputValues></output><output name="route"><outputValues><text>"A", "B"</text></outputValues></output><rule><inputEntry><text>-</text></inputEntry><outputEntry><text>"low"</text></outputEntry><outputEntry><text>"A"</text></outputEntry></rule><rule><inputEntry><text>-</text></inputEntry><outputEntry><text>"high"</text></outputEntry><outputEntry><text>"B"</text></outputEntry></rule></decisionTable>"#,
    );
    assert_eq!(
        DmnExecutor::new(&model)
            .evaluate(
                "result",
                HashMap::from([("age".into(), FeelValue::Number(20.0))]),
            )
            .unwrap()
            .value,
        FeelValue::Context(HashMap::from([
            ("risk".into(), FeelValue::String("high".into())),
            ("route".into(), FeelValue::String("B".into())),
        ]))
    );
}

#[test]
fn dmn15_input_values_constrain_dash_and_no_hit_returns_null() {
    let constrained = model(
        r#"<decisionTable hitPolicy="COLLECT" aggregation="COUNT"><input><inputExpression><text>age</text></inputExpression><inputValues><text>[1..10]</text></inputValues></input><output name="result"/><rule><inputEntry><text>-</text></inputEntry><outputEntry><text>1</text></outputEntry></rule></decisionTable>"#,
    );
    let executor = DmnExecutor::new(&constrained);
    assert_eq!(
        executor
            .evaluate(
                "result",
                HashMap::from([("age".into(), FeelValue::Number(5.0))]),
            )
            .unwrap()
            .value,
        FeelValue::Number(1.0)
    );
    assert_eq!(
        executor
            .evaluate(
                "result",
                HashMap::from([("age".into(), FeelValue::Number(20.0))]),
            )
            .unwrap()
            .value,
        FeelValue::Null
    );

    let explicit_rule = model(
        r#"<decisionTable><input><inputExpression><text>age</text></inputExpression><inputValues><text>[1..10]</text></inputValues></input><output name="result"/><rule><inputEntry><text>&gt; 0</text></inputEntry><outputEntry><text>"matched"</text></outputEntry></rule></decisionTable>"#,
    );
    assert_eq!(
        DmnExecutor::new(&explicit_rule)
            .evaluate(
                "result",
                HashMap::from([("age".into(), FeelValue::Number(20.0))]),
            )
            .unwrap()
            .value,
        FeelValue::Null
    );

    let nullable = model(
        r#"<decisionTable><input><inputExpression><text>age</text></inputExpression><inputValues><text>null</text></inputValues></input><output name="result"/><rule><inputEntry><text>-</text></inputEntry><outputEntry><text>"null accepted"</text></outputEntry></rule></decisionTable>"#,
    );
    assert_eq!(
        DmnExecutor::new(&nullable)
            .evaluate("result", HashMap::from([("age".into(), FeelValue::Null)]))
            .unwrap()
            .value,
        FeelValue::String("null accepted".into())
    );

    let compound = model(
        r#"<decisionTable><input><inputExpression><text>age</text></inputExpression></input><output name="risk"/><output name="limit"/><rule><inputEntry><text>>= 18</text></inputEntry><outputEntry><text>"high"</text></outputEntry><outputEntry><text>100</text></outputEntry></rule></decisionTable>"#,
    );
    assert_eq!(
        DmnExecutor::new(&compound)
            .evaluate(
                "result",
                HashMap::from([("age".into(), FeelValue::Number(10.0))]),
            )
            .unwrap()
            .value,
        FeelValue::Null
    );
}

#[test]
fn dmn15_output_values_constrain_rule_and_default_outputs() {
    let table = |rule_output: &str, default_output: &str| {
        model(&format!(
            r#"<decisionTable>
                <input><inputExpression><text>age</text></inputExpression></input>
                <output name="result"><outputValues><text>"allowed", "fallback"</text></outputValues><defaultOutputEntry><text>{default_output}</text></defaultOutputEntry></output>
                <rule><inputEntry><text>&gt;= 18</text></inputEntry><outputEntry><text>{rule_output}</text></outputEntry></rule>
            </decisionTable>"#
        ))
    };

    let valid = table("\"allowed\"", "\"fallback\"");
    let executor = DmnExecutor::new(&valid);
    assert_eq!(
        executor
            .evaluate(
                "result",
                HashMap::from([("age".into(), FeelValue::Number(20.0))])
            )
            .unwrap()
            .value,
        FeelValue::String("allowed".into())
    );
    assert_eq!(
        executor
            .evaluate(
                "result",
                HashMap::from([("age".into(), FeelValue::Number(10.0))])
            )
            .unwrap()
            .value,
        FeelValue::String("fallback".into())
    );

    for (model, age) in [
        (table("\"invalid\"", "\"fallback\""), 20.0),
        (table("\"allowed\"", "\"invalid\""), 10.0),
    ] {
        let result = DmnExecutor::new(&model).evaluate_by_id(
            "result",
            HashMap::from([("age".into(), FeelValue::Number(age))]),
        );
        assert!(matches!(
            result.first_error().and_then(|message| message.error.as_ref()),
            Some(DmnError::TypeConstraintViolation { location, type_ref, detail })
                if location == "result.result"
                    && type_ref == "outputValues"
                    && detail.contains("outside outputValues")
        ));
    }
}

#[test]
fn dmn15_context_unnamed_final_entry_is_the_context_result() {
    let model = parse_dmn(&format!(
        r#"<definitions xmlns="{DMN_NAMESPACE}"><decision id="result" name="result"><context><contextEntry><variable name="base"/><literalExpression><text>40</text></literalExpression></contextEntry><contextEntry><literalExpression><text>base + 2</text></literalExpression></contextEntry></context></decision></definitions>"#
    ))
    .unwrap();
    assert_eq!(
        DmnExecutor::new(&model)
            .evaluate("result", HashMap::new())
            .unwrap()
            .value,
        FeelValue::Number(42.0)
    );

    let serialized = model.to_dmn_xml().unwrap();
    assert!(!serialized.contains("<variable name=\"\""));
    assert_eq!(
        DmnExecutor::new(&parse_dmn(&serialized).unwrap())
            .evaluate("result", HashMap::new())
            .unwrap()
            .value,
        FeelValue::Number(42.0)
    );
}

#[test]
fn dmn15_zero_input_decision_table_rule_is_always_applicable() {
    let model = parse_dmn(&format!(
        r#"<definitions xmlns="{DMN_NAMESPACE}"><decision id="result" name="result"><decisionTable><output name="result"/><rule id="constant"><outputEntry><text>42</text></outputEntry></rule></decisionTable></decision></definitions>"#,
    ))
    .unwrap();
    let result = DmnExecutor::new(&model)
        .evaluate("result", HashMap::new())
        .unwrap();
    assert_eq!(result.value, FeelValue::Number(42.0));
    assert_eq!(result.matched_rule_ids, vec!["constant"]);
}

#[test]
fn diagnostics_filter_by_severity_and_preserve_typed_causes() {
    let model = model(
        r#"<decisionTable id="table"><input><inputExpression><text>age</text></inputExpression></input><output name="result"/><rule id="broken"><inputEntry><text>-</text></inputEntry><outputEntry><text>missing</text></outputEntry></rule></decisionTable>"#,
    );
    let result = DmnExecutor::new(&model).evaluate_by_id(
        "result",
        HashMap::from([("age".into(), FeelValue::Number(20.0))]),
    );
    assert!(result.has_errors());
    assert!(!result.has_warnings());
    assert_eq!(result.error_count(), 1);
    assert_eq!(result.warning_count(), 0);
    assert_eq!(result.messages().len(), 1);
    let message = result.first_error().unwrap();
    assert_eq!(message.code, "DMN-EVAL-FEEL");
    assert_eq!(message.source.element_id.as_deref(), Some("broken"));
    assert_eq!(message.source.rule_id.as_deref(), Some("broken"));
    assert!(matches!(message.error, Some(DmnError::Feel { .. })));
    assert_eq!(result.errors().count(), 1);
    assert_eq!(result.warnings().count(), 0);
}

#[test]
fn table_level_diagnostics_use_the_table_source() {
    let model = model(
        r#"<decisionTable id="table" hitPolicy="UNIQUE"><input><inputExpression><text>age</text></inputExpression></input><output name="result"/><rule id="first"><inputEntry><text>>= 10</text></inputEntry><outputEntry><text>1</text></outputEntry></rule><rule id="second"><inputEntry><text>>= 18</text></inputEntry><outputEntry><text>2</text></outputEntry></rule></decisionTable>"#,
    );
    let result = DmnExecutor::new(&model).evaluate_by_id(
        "result",
        HashMap::from([("age".into(), FeelValue::Number(20.0))]),
    );
    let message = result.first_error().unwrap();
    assert_eq!(message.code, "DMN-EVAL-HIT-POLICY");
    assert_eq!(message.source.element_id.as_deref(), Some("table"));
    assert_eq!(message.source.rule_id, None);
}

#[test]
fn validates_local_item_definition_allowed_values() {
    let model = parse_dmn(TYPED_ALLOWED_VALUES).unwrap();
    assert_eq!(model.item_definitions.len(), 1);
    assert_eq!(
        model.input_data[0].type_ref.as_deref(),
        Some("EmploymentStatus")
    );
    assert_eq!(model.decisions[0].type_ref.as_deref(), Some("string"));

    let executor = DmnExecutor::new(&model);
    assert_eq!(
        executor
            .evaluate(
                "result",
                HashMap::from([("employment".into(), FeelValue::String("EMPLOYED".into()))])
            )
            .unwrap()
            .value,
        FeelValue::String("EMPLOYED".into())
    );
    let result = executor.evaluate_by_name(
        "result",
        HashMap::from([("employment".into(), FeelValue::String("RETIRED".into()))]),
    );
    let message = result.first_error().unwrap();
    assert_eq!(message.code, "DMN-EVAL-TYPE");
    assert!(matches!(
        message.error,
        Some(DmnError::TypeConstraintViolation { .. })
    ));
}

#[test]
fn dmn15_item_allowed_values_must_explicitly_allow_null() {
    let model_with_values = |allowed_values: &str| {
        parse_dmn(&format!(
            r##"<definitions xmlns="{DMN_NAMESPACE}"><itemDefinition name="Status"><typeRef>string</typeRef><allowedValues><text>{allowed_values}</text></allowedValues></itemDefinition><inputData id="status-input" name="status"><variable name="status" typeRef="Status"/></inputData><decision id="result" name="result"><informationRequirement><requiredInput href="#status-input"/></informationRequirement><literalExpression><text>status</text></literalExpression></decision></definitions>"##
        ))
        .unwrap()
    };

    let rejects_null = model_with_values("\"ACTIVE\"");
    let result = DmnExecutor::new(&rejects_null).evaluate_by_id(
        "result",
        HashMap::from([("status".into(), FeelValue::Null)]),
    );
    assert!(matches!(
        result.first_error().and_then(|message| message.error.as_ref()),
        Some(DmnError::TypeConstraintViolation { detail, .. })
            if detail.contains("outside allowedValues")
    ));

    let accepts_null = model_with_values("\"ACTIVE\", null");
    assert_eq!(
        DmnExecutor::new(&accepts_null)
            .evaluate(
                "result",
                HashMap::from([("status".into(), FeelValue::Null)]),
            )
            .unwrap()
            .value,
        FeelValue::Null
    );
}

#[test]
fn rejects_invalid_local_type_definitions() {
    let duplicate = parse_dmn(&format!(
        r#"<definitions xmlns="{DMN_NAMESPACE}"><itemDefinition name="Status"/><itemDefinition name="Status"/></definitions>"#
    ))
    .unwrap();
    assert!(matches!(
        compile_dmn(&duplicate),
        Err(DmnError::InvalidTypeDefinition { .. })
    ));

    let unknown = parse_dmn(&format!(
        r#"<definitions xmlns="{DMN_NAMESPACE}"><inputData id="input" name="input"><variable name="input" typeRef="Missing"/></inputData></definitions>"#
    ))
    .unwrap();
    assert!(matches!(
        compile_dmn(&unknown),
        Err(DmnError::UnknownTypeReference { .. })
    ));
}

#[test]
fn dmn15_decision_service_parses_round_trips_and_exposes_only_outputs() {
    let xml = format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}" name="service-model">
            <inputData id="input-x" name="x"><variable name="x" typeRef="number"/></inputData>
            <decision id="internal" name="internal">
                <informationRequirement><requiredInput href="#input-x"/></informationRequirement>
                <literalExpression><text>x * 2</text></literalExpression>
            </decision>
            <decision id="output" name="output">
                <informationRequirement><requiredDecision href="#internal"/></informationRequirement>
                <informationRequirement><requiredDecision href="#external"/></informationRequirement>
                <literalExpression><text>internal + external</text></literalExpression>
            </decision>
            <decision id="external" name="external"><variable name="external" typeRef="number"/><literalExpression><text>100</text></literalExpression></decision>
            <decision id="unrelated" name="unrelated"><literalExpression><text>999</text></literalExpression></decision>
            <decisionService id="service" name="Calculation Service">
                <outputDecision href="#output"/>
                <encapsulatedDecision href="#internal"/>
                <inputDecision href="#external"/>
                <inputData href="#input-x"/>
            </decisionService>
        </definitions>"##
    );
    let model = parse_dmn(&xml).unwrap();
    let service = &model.decision_services[0];
    assert_eq!(service.output_decisions, ["output"]);
    assert_eq!(service.encapsulated_decisions, ["internal"]);
    assert_eq!(service.input_decisions, ["external"]);
    assert_eq!(service.input_data, ["input-x"]);

    let serialized = model.to_dmn_xml().unwrap();
    let reparsed = parse_dmn(&serialized).unwrap();
    assert_eq!(reparsed.decision_services[0].output_decisions, ["output"]);
    assert_eq!(
        reparsed.decision_services[0].encapsulated_decisions,
        ["internal"]
    );

    let compiled = compile_dmn(&model).unwrap();
    let executor = DmnExecutor::from_compiled(&compiled);
    let result = executor.evaluate_decision_service(
        "Calculation Service",
        HashMap::from([
            ("x".into(), FeelValue::Number(20.0)),
            ("external".into(), FeelValue::Number(5.0)),
        ]),
    );
    assert_eq!(result.status, DmnResultStatus::Succeeded);
    assert_eq!(result.decision_results.len(), 1);
    assert_eq!(
        result.decision_by_id("output").unwrap().value,
        FeelValue::Number(45.0)
    );
    assert!(result.decision_by_id("internal").is_none());
    assert!(result.decision_by_id("unrelated").is_none());
    assert_eq!(
        result
            .decision_by_id("output")
            .unwrap()
            .evaluated_dependencies,
        ["internal", "external"]
    );

    let detailed = executor.evaluate_decision_service_with_details(
        "service",
        HashMap::from([
            ("x".into(), FeelValue::Number(20.0)),
            ("external".into(), FeelValue::Number(5.0)),
        ]),
    );
    assert_eq!(detailed.status, DmnResultStatus::Succeeded);
    assert!(detailed.decision_by_id("internal").is_some());
    assert!(detailed.decision_by_id("external").is_some());
    assert!(detailed.decision_by_id("output").is_some());
    assert!(detailed.decision_by_id("unrelated").is_none());

    let owned = compile_dmn_xml(&xml).unwrap();
    assert_eq!(
        owned
            .evaluate_decision_service(
                "service",
                HashMap::from([
                    ("x".into(), FeelValue::Number(2.0)),
                    ("external".into(), FeelValue::Number(3.0)),
                ]),
            )
            .decision_by_id("output")
            .unwrap()
            .value,
        FeelValue::Number(7.0)
    );

    let missing_input_decision = executor.evaluate_decision_service(
        "service",
        HashMap::from([("x".into(), FeelValue::Number(2.0))]),
    );
    assert!(matches!(
        missing_input_decision
            .first_error()
            .and_then(|message| message.error.as_ref()),
        Some(DmnError::MissingInput(input)) if input == "external"
    ));
    let invalid_input_decision = executor.evaluate_decision_service(
        "service",
        HashMap::from([
            ("x".into(), FeelValue::Number(2.0)),
            ("external".into(), FeelValue::String("invalid".into())),
        ]),
    );
    assert!(matches!(
        invalid_input_decision
            .first_error()
            .and_then(|message| message.error.as_ref()),
        Some(DmnError::TypeConstraintViolation { location, .. }) if location == "external"
    ));
}

#[test]
fn dmn15_decision_service_references_are_validated_and_errors_are_structured() {
    let unknown = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}"><decisionService id="service" name="service"><outputDecision href="#missing"/></decisionService></definitions>"##
    ))
    .unwrap();
    assert!(matches!(
        compile_dmn(&unknown),
        Err(DmnError::InvalidDecisionService { service, detail })
            if service == "service" && detail.contains("unknown decision")
    ));

    let duplicate_role = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}"><decision id="result" name="result"><literalExpression><text>1</text></literalExpression></decision><decisionService id="service" name="service"><outputDecision href="#result"/><encapsulatedDecision href="#result"/></decisionService></definitions>"##
    ))
    .unwrap();
    assert!(matches!(
        compile_dmn(&duplicate_role),
        Err(DmnError::InvalidDecisionService { detail, .. }) if detail.contains("listed as both")
    ));

    let empty = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}"><decisionService id="empty" name="Empty"/></definitions>"##
    ))
    .unwrap();
    let executor = DmnExecutor::new(&empty);
    let result = executor.evaluate_decision_service("Empty", HashMap::new());
    assert_eq!(result.status, DmnResultStatus::Failed);
    assert!(matches!(
        result.first_error().and_then(|message| message.error.as_ref()),
        Some(DmnError::InvalidDecisionService { detail, .. }) if detail.contains("no outputDecision")
    ));
    assert!(matches!(
        executor
            .evaluate_decision_service("missing", HashMap::new())
            .first_error()
            .and_then(|message| message.error.as_ref()),
        Some(DmnError::UnknownDecisionService(service)) if service == "missing"
    ));
}

#[test]
fn dmn15_composite_item_definitions_require_all_declared_components() {
    let xml = format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}">
            <itemDefinition name="Customer">
                <itemComponent name="age"><typeRef>number</typeRef></itemComponent>
                <itemComponent name="name"><typeRef>string</typeRef></itemComponent>
            </itemDefinition>
            <inputData id="customer-input" name="customer"><variable name="customer" typeRef="Customer"/></inputData>
            <decision id="result" name="result">
                <informationRequirement><requiredInput href="#customer-input"/></informationRequirement>
                <literalExpression><text>customer.name</text></literalExpression>
            </decision>
        </definitions>"##
    );
    let model = parse_dmn(&xml).unwrap();
    let executor = DmnExecutor::new(&model);
    let valid = executor.evaluate(
        "result",
        HashMap::from([(
            "customer".into(),
            FeelValue::Context(HashMap::from([
                ("age".into(), FeelValue::Number(30.0)),
                ("name".into(), FeelValue::String("Alice".into())),
            ])),
        )]),
    );
    assert_eq!(valid.unwrap().value, FeelValue::String("Alice".into()));

    let missing = executor.evaluate_by_name(
        "result",
        HashMap::from([(
            "customer".into(),
            FeelValue::Context(HashMap::from([(
                "name".into(),
                FeelValue::String("Alice".into()),
            )])),
        )]),
    );
    let error = missing.first_error().unwrap();
    assert_eq!(error.code, "DMN-EVAL-TYPE");
    assert!(matches!(
        error.error.as_ref(),
        Some(DmnError::TypeConstraintViolation { detail, .. })
            if detail.contains("missing component 'age'")
    ));
}

#[test]
fn dmn15_relation_expression_returns_typed_context_rows_and_round_trips() {
    let xml = format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}">
            <inputData id="base-input" name="base"><variable name="base" typeRef="number"/></inputData>
            <decision id="people" name="people">
                <informationRequirement><requiredInput href="#base-input"/></informationRequirement>
                <relation id="people-relation">
                    <column id="name-column" name="name" typeRef="string"/>
                    <column id="score-column" name="score" typeRef="number"/>
                    <row id="alice-row">
                        <literalExpression><text>"Alice"</text></literalExpression>
                        <literalExpression><text>base + 1</text></literalExpression>
                    </row>
                    <row id="empty-row">
                        <literalExpression/>
                        <literalExpression><text>base + 2</text></literalExpression>
                    </row>
                </relation>
            </decision>
        </definitions>"##
    );
    let model = parse_dmn(&xml).unwrap();
    let DecisionLogic::Relation(relation) = &model.decisions[0].logic else {
        panic!("expected relation expression");
    };
    assert_eq!(relation.columns.len(), 2);
    assert_eq!(relation.rows.len(), 2);
    assert!(matches!(
        &relation.rows[1].entries[0],
        DecisionLogic::LiteralExpression(expression) if expression == "null"
    ));

    let value = DmnExecutor::new(&model)
        .evaluate(
            "people",
            HashMap::from([("base".into(), FeelValue::Number(10.0))]),
        )
        .unwrap()
        .value;
    assert_eq!(
        value,
        FeelValue::List(vec![
            FeelValue::Context(HashMap::from([
                ("name".into(), FeelValue::String("Alice".into())),
                ("score".into(), FeelValue::Number(11.0)),
            ])),
            FeelValue::Context(HashMap::from([
                ("name".into(), FeelValue::Null),
                ("score".into(), FeelValue::Number(12.0)),
            ])),
        ])
    );

    let serialized = model.to_dmn_xml().unwrap();
    let reparsed = parse_dmn(&serialized).unwrap();
    assert!(matches!(
        reparsed.decisions[0].logic,
        DecisionLogic::Relation(_)
    ));
}

#[test]
fn dmn15_relation_shape_and_column_types_are_validated() {
    let duplicate_columns = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}"><decision id="bad" name="bad"><relation><column name="value"/><column name="value"/><row><literalExpression><text>1</text></literalExpression><literalExpression><text>2</text></literalExpression></row></relation></decision></definitions>"##
    ))
    .unwrap();
    assert!(matches!(
        compile_dmn(&duplicate_columns),
        Err(DmnError::Feel { detail, .. }) if detail.contains("duplicate relation column")
    ));

    let wrong_width = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}"><decision id="bad" name="bad"><relation><column name="a"/><column name="b"/><row><literalExpression><text>1</text></literalExpression></row></relation></decision></definitions>"##
    ))
    .unwrap();
    assert!(matches!(
        compile_dmn(&wrong_width),
        Err(DmnError::Feel { detail, .. }) if detail.contains("entry count")
    ));

    let wrong_type = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}"><decision id="bad" name="bad"><relation><column name="score" typeRef="number"/><row><literalExpression><text>"not a number"</text></literalExpression></row></relation></decision></definitions>"##
    ))
    .unwrap();
    let result = DmnExecutor::new(&wrong_type).evaluate_by_id("bad", HashMap::new());
    assert!(matches!(
        result.first_error().and_then(|message| message.error.as_ref()),
        Some(DmnError::TypeConstraintViolation { location, .. })
            if location == "bad.relation[1].score"
    ));
}

#[test]
fn dmn15_encapsulated_logic_and_knowledge_requirements_execute_and_round_trip() {
    let xml = format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}">
            <businessKnowledgeModel id="increment" name="increment">
                <variable name="increment"/>
                <encapsulatedLogic>
                    <formalParameter name="value"/>
                    <literalExpression><text>value + 1</text></literalExpression>
                </encapsulatedLogic>
            </businessKnowledgeModel>
            <decision id="result" name="result">
                <knowledgeRequirement id="knowledge-link"><requiredKnowledge href="#increment"/></knowledgeRequirement>
                <invocation>
                    <literalExpression><text>increment</text></literalExpression>
                    <binding><parameter name="value"/><literalExpression><text>41</text></literalExpression></binding>
                </invocation>
            </decision>
        </definitions>"##
    );
    let model = parse_dmn(&xml).unwrap();
    assert_eq!(
        model.decisions[0].knowledge_requirements[0].required_knowledge,
        "increment"
    );
    assert!(matches!(
        model.business_knowledge_models[0].logic,
        DecisionLogic::FunctionDefinition(_)
    ));
    assert_eq!(
        DmnExecutor::new(&model)
            .evaluate("result", HashMap::new())
            .unwrap()
            .value,
        FeelValue::Number(42.0)
    );

    let restored = parse_dmn(&model.to_dmn_xml().unwrap()).unwrap();
    assert_eq!(restored.decisions[0].knowledge_requirements.len(), 1);
    assert_eq!(
        DmnExecutor::new(&restored)
            .evaluate("result", HashMap::new())
            .unwrap()
            .value,
        FeelValue::Number(42.0)
    );
}

#[test]
fn dmn15_knowledge_requirement_binds_bkm_for_direct_feel_calls() {
    let xml = format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}">
            <businessKnowledgeModel id="limit" name="limit">
                <variable name="limit"/>
                <encapsulatedLogic><literalExpression><text>0.5</text></literalExpression></encapsulatedLogic>
            </businessKnowledgeModel>
            <decision id="result" name="result">
                <knowledgeRequirement><requiredKnowledge href="#limit"/></knowledgeRequirement>
                <literalExpression><text>if limit() = 0.5 then "ok" else "bad"</text></literalExpression>
            </decision>
        </definitions>"##
    );
    let model = parse_dmn(&xml).unwrap();
    assert_eq!(
        DmnExecutor::new(&model)
            .evaluate("result", HashMap::new())
            .unwrap()
            .value,
        FeelValue::String("ok".into())
    );
}

#[test]
fn dmn15_direct_feel_bkm_calls_bind_transitive_knowledge_requirements() {
    let xml = format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}">
            <businessKnowledgeModel id="base" name="base">
                <encapsulatedLogic><literalExpression><text>40</text></literalExpression></encapsulatedLogic>
            </businessKnowledgeModel>
            <businessKnowledgeModel id="increment" name="increment">
                <knowledgeRequirement><requiredKnowledge href="#base"/></knowledgeRequirement>
                <encapsulatedLogic><literalExpression><text>base() + 2</text></literalExpression></encapsulatedLogic>
            </businessKnowledgeModel>
            <decision id="result" name="result">
                <knowledgeRequirement><requiredKnowledge href="#increment"/></knowledgeRequirement>
                <literalExpression><text>increment()</text></literalExpression>
            </decision>
        </definitions>"##
    );
    let model = parse_dmn(&xml).unwrap();
    assert_eq!(
        DmnExecutor::new(&model)
            .evaluate("result", HashMap::new())
            .unwrap()
            .value,
        FeelValue::Number(42.0)
    );
}

#[test]
fn dmn15_invocation_bkm_calls_bind_its_knowledge_requirements() {
    let xml = format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}">
            <businessKnowledgeModel id="base" name="base">
                <encapsulatedLogic><literalExpression><text>40</text></literalExpression></encapsulatedLogic>
            </businessKnowledgeModel>
            <businessKnowledgeModel id="increment" name="increment">
                <knowledgeRequirement><requiredKnowledge href="#base"/></knowledgeRequirement>
                <encapsulatedLogic><formalParameter name="amount"/><literalExpression><text>base() + amount</text></literalExpression></encapsulatedLogic>
            </businessKnowledgeModel>
            <decision id="result" name="result">
                <knowledgeRequirement><requiredKnowledge href="#increment"/></knowledgeRequirement>
                <invocation><literalExpression><text>increment</text></literalExpression><binding><parameter name="amount"/><literalExpression><text>2</text></literalExpression></binding></invocation>
            </decision>
        </definitions>"##
    );
    let model = parse_dmn(&xml).unwrap();
    assert_eq!(
        DmnExecutor::new(&model)
            .evaluate("result", HashMap::new())
            .unwrap()
            .value,
        FeelValue::Number(42.0)
    );
}

#[test]
fn dmn15_direct_feel_bkm_bindings_do_not_leak_between_decisions() {
    let xml = format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}">
            <businessKnowledgeModel id="limit" name="limit">
                <encapsulatedLogic><literalExpression><text>1</text></literalExpression></encapsulatedLogic>
            </businessKnowledgeModel>
            <decision id="bound" name="bound">
                <knowledgeRequirement><requiredKnowledge href="#limit"/></knowledgeRequirement>
                <literalExpression><text>limit()</text></literalExpression>
            </decision>
            <decision id="unbound" name="unbound">
                <literalExpression><text>limit()</text></literalExpression>
            </decision>
        </definitions>"##
    );
    let model = parse_dmn(&xml).unwrap();
    let result = DmnExecutor::new(&model).evaluate_all(HashMap::new());
    assert_eq!(result.decision_results[0].status, DecisionStatus::Succeeded);
    assert_eq!(result.decision_results[1].status, DecisionStatus::Failed);
    assert!(result.messages.iter().any(|message| {
        message
            .error
            .as_ref()
            .is_some_and(|error| matches!(error, DmnError::Feel { detail, .. } if detail.contains("Unknown function: limit")))
    }));
}

#[test]
fn dmn15_bkm_encapsulated_logic_supports_non_literal_bodies() {
    let xml = format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}">
            <businessKnowledgeModel id="people" name="people">
                <encapsulatedLogic>
                    <relation>
                        <column name="name" typeRef="string"/>
                        <row><literalExpression><text>"Alice"</text></literalExpression></row>
                    </relation>
                </encapsulatedLogic>
            </businessKnowledgeModel>
            <decision id="result" name="result">
                <knowledgeRequirement><requiredKnowledge href="#people"/></knowledgeRequirement>
                <invocation><literalExpression><text>people</text></literalExpression></invocation>
            </decision>
        </definitions>"##
    );
    let model = parse_dmn(&xml).unwrap();
    assert_eq!(
        DmnExecutor::new(&model)
            .evaluate("result", HashMap::new())
            .unwrap()
            .value,
        FeelValue::List(vec![FeelValue::Context(HashMap::from([(
            "name".into(),
            FeelValue::String("Alice".into()),
        )]))])
    );
}

#[test]
fn dmn15_knowledge_references_and_cycles_are_rejected() {
    let unknown = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}"><decision id="result" name="result"><knowledgeRequirement><requiredKnowledge href="#missing"/></knowledgeRequirement><literalExpression><text>1</text></literalExpression></decision></definitions>"##
    ))
    .unwrap();
    assert!(matches!(
        compile_dmn(&unknown),
        Err(DmnError::UnknownReference { reference, .. }) if reference == "missing"
    ));

    let cycle = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}">
            <businessKnowledgeModel id="a" name="a"><knowledgeRequirement><requiredKnowledge href="#b"/></knowledgeRequirement><encapsulatedLogic><literalExpression><text>1</text></literalExpression></encapsulatedLogic></businessKnowledgeModel>
            <businessKnowledgeModel id="b" name="b"><knowledgeRequirement><requiredKnowledge href="#a"/></knowledgeRequirement><encapsulatedLogic><literalExpression><text>2</text></literalExpression></encapsulatedLogic></businessKnowledgeModel>
        </definitions>"##
    ))
    .unwrap();
    assert!(matches!(
        compile_dmn(&cycle),
        Err(DmnError::CircularDependency(id)) if id == "a" || id == "b"
    ));

    assert!(matches!(
        parse_dmn(&format!(
            r##"<definitions xmlns="{DMN_NAMESPACE}"><decision id="result" name="result"><knowledgeRequirement/><literalExpression><text>1</text></literalExpression></decision></definitions>"##
        )),
        Err(DmnError::Xml(detail)) if detail.contains("missing requiredKnowledge")
    ));
}

#[test]
fn dmn15_boxed_expressions_execute_with_scoped_iterators_and_round_trip() {
    let xml = format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}">
            <decision id="conditional" name="conditional">
                <conditional>
                    <if><literalExpression><text>1</text></literalExpression></if>
                    <then><literalExpression><text>"wrong"</text></literalExpression></then>
                    <else><literalExpression><text>"fallback"</text></literalExpression></else>
                </conditional>
            </decision>
            <decision id="filter-predicate" name="filter predicate">
                <filter>
                    <in><literalExpression><text>[1, 2, 3, 4]</text></literalExpression></in>
                    <match><literalExpression><text>item &gt; 2</text></literalExpression></match>
                </filter>
            </decision>
            <decision id="filter-index" name="filter index">
                <filter>
                    <in><literalExpression><text>[1, 2, 3, 4]</text></literalExpression></in>
                    <match><literalExpression><text>-1</text></literalExpression></match>
                </filter>
            </decision>
            <decision id="filter-context" name="filter context">
                <filter>
                    <in><literalExpression><text>[{{score: 1}}, {{score: 3}}]</text></literalExpression></in>
                    <match><literalExpression><text>score &gt; 1</text></literalExpression></match>
                </filter>
            </decision>
            <decision id="for-list" name="for list">
                <for iteratorVariable="x">
                    <in><literalExpression><text>[1, 2, 3]</text></literalExpression></in>
                    <return><literalExpression><text>x * 2</text></literalExpression></return>
                </for>
            </decision>
            <decision id="for-range" name="for range">
                <for iteratorVariable="x">
                    <in><literalExpression><text>(0..4)</text></literalExpression></in>
                    <return><literalExpression><text>x</text></literalExpression></return>
                </for>
            </decision>
            <decision id="for-partial" name="for partial">
                <for iteratorVariable="x">
                    <in><literalExpression><text>[1, 2, 3]</text></literalExpression></in>
                    <return><literalExpression><text>if x = 1 then x else x + partial[-1]</text></literalExpression></return>
                </for>
            </decision>
            <decision id="some-unknown" name="some unknown">
                <some iteratorVariable="x">
                    <in><literalExpression><text>[1, 2]</text></literalExpression></in>
                    <satisfies><literalExpression><text>if x = 1 then null else false</text></literalExpression></satisfies>
                </some>
            </decision>
            <decision id="some-empty" name="some empty">
                <some iteratorVariable="x">
                    <in><literalExpression><text>[]</text></literalExpression></in>
                    <satisfies><literalExpression><text>true</text></literalExpression></satisfies>
                </some>
            </decision>
            <decision id="every-unknown" name="every unknown">
                <every iteratorVariable="x">
                    <in><literalExpression><text>[1, 2]</text></literalExpression></in>
                    <satisfies><literalExpression><text>if x = 1 then null else true</text></literalExpression></satisfies>
                </every>
            </decision>
            <decision id="every-empty" name="every empty">
                <every iteratorVariable="x">
                    <in><literalExpression><text>[]</text></literalExpression></in>
                    <satisfies><literalExpression><text>false</text></literalExpression></satisfies>
                </every>
            </decision>
        </definitions>"##
    );
    let expected = vec![
        ("conditional", FeelValue::String("fallback".into())),
        (
            "filter-predicate",
            FeelValue::List(vec![FeelValue::Number(3.0), FeelValue::Number(4.0)]),
        ),
        ("filter-index", FeelValue::Number(4.0)),
        (
            "filter-context",
            FeelValue::List(vec![FeelValue::Context(HashMap::from([(
                "score".into(),
                FeelValue::Number(3.0),
            )]))]),
        ),
        (
            "for-list",
            FeelValue::List(vec![
                FeelValue::Number(2.0),
                FeelValue::Number(4.0),
                FeelValue::Number(6.0),
            ]),
        ),
        (
            "for-range",
            FeelValue::List(vec![
                FeelValue::Number(1.0),
                FeelValue::Number(2.0),
                FeelValue::Number(3.0),
            ]),
        ),
        (
            "for-partial",
            FeelValue::List(vec![
                FeelValue::Number(1.0),
                FeelValue::Number(3.0),
                FeelValue::Number(6.0),
            ]),
        ),
        ("some-unknown", FeelValue::Null),
        ("some-empty", FeelValue::Bool(false)),
        ("every-unknown", FeelValue::Null),
        ("every-empty", FeelValue::Bool(true)),
    ];

    let model = parse_dmn(&xml).unwrap();
    for model in [&model, &parse_dmn(&model.to_dmn_xml().unwrap()).unwrap()] {
        let executor = DmnExecutor::new(model);
        for (decision, expected) in &expected {
            assert_eq!(
                executor
                    .evaluate(decision, HashMap::new())
                    .unwrap_or_else(|error| panic!("{decision}: {error}"))
                    .value,
                *expected,
                "{decision}"
            );
        }
    }
}

#[test]
fn dmn15_boxed_expression_structure_is_validated() {
    assert!(matches!(
        parse_dmn(&format!(
            r##"<definitions xmlns="{DMN_NAMESPACE}"><decision id="bad" name="bad"><conditional><if><literalExpression><text>true</text></literalExpression></if><then><literalExpression><text>1</text></literalExpression></then></conditional></decision></definitions>"##
        )),
        Err(DmnError::Xml(detail)) if detail.contains("missing else")
    ));
    assert!(matches!(
        parse_dmn(&format!(
            r##"<definitions xmlns="{DMN_NAMESPACE}"><decision id="bad" name="bad"><filter><in><literalExpression><text>[]</text></literalExpression></in></filter></decision></definitions>"##
        )),
        Err(DmnError::Xml(detail)) if detail.contains("missing match")
    ));

    let empty_iterator = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}"><decision id="bad" name="bad"><for iteratorVariable=""><in><literalExpression><text>[]</text></literalExpression></in><return><literalExpression><text>1</text></literalExpression></return></for></decision></definitions>"##
    ))
    .unwrap();
    assert!(matches!(
        compile_dmn(&empty_iterator),
        Err(DmnError::Feel { detail, .. }) if detail.contains("iteratorVariable")
    ));
}

#[test]
fn dmn15_list_and_invocation_bindings_survive_xml_round_trip() {
    let model = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}">
            <businessKnowledgeModel id="increment" name="increment">
                <encapsulatedLogic><formalParameter name="x"/><literalExpression><text>x + 1</text></literalExpression></encapsulatedLogic>
            </businessKnowledgeModel>
            <decision id="result" name="result">
                <knowledgeRequirement><requiredKnowledge href="#increment"/></knowledgeRequirement>
                <list>
                    <literalExpression><text>1</text></literalExpression>
                    <invocation><literalExpression><text>increment</text></literalExpression><binding><parameter name="x"/><literalExpression><text>1</text></literalExpression></binding></invocation>
                </list>
            </decision>
        </definitions>"##
    ))
    .unwrap();
    let restored = parse_dmn(&model.to_dmn_xml().unwrap()).unwrap();
    let DecisionLogic::List(items) = &restored.decisions[0].logic else {
        panic!("expected list expression");
    };
    assert_eq!(items.len(), 2);
    let DecisionLogic::Invocation(invocation) = &items[1] else {
        panic!("expected invocation expression");
    };
    assert_eq!(invocation.bindings.len(), 1);
    assert_eq!(invocation.bindings[0].parameter, "x");
    assert_eq!(
        DmnExecutor::new(&restored)
            .evaluate("result", HashMap::new())
            .unwrap()
            .value,
        FeelValue::List(vec![FeelValue::Number(1.0), FeelValue::Number(2.0)])
    );
}

#[test]
fn invocation_can_fill_a_missing_binding_from_same_named_input_context() {
    let model = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}">
            <inputData id="cost" name="EstimatedMonthlyCost"><variable name="EstimatedMonthlyCost" typeRef="number"/></inputData>
            <inputData id="region" name="Region"><variable name="Region" typeRef="string"/></inputData>
            <businessKnowledgeModel id="score" name="score">
                <encapsulatedLogic><formalParameter name="EstimatedMonthlyCost"/><formalParameter name="Region"/><literalExpression><text>EstimatedMonthlyCost / 100</text></literalExpression></encapsulatedLogic>
            </businessKnowledgeModel>
            <decision id="result" name="result">
                <informationRequirement><requiredInput href="#cost"/></informationRequirement>
                <informationRequirement><requiredInput href="#region"/></informationRequirement>
                <knowledgeRequirement><requiredKnowledge href="#score"/></knowledgeRequirement>
                <invocation><literalExpression><text>score</text></literalExpression></invocation>
            </decision>
        </definitions>"##
    ))
    .unwrap();
    let result = DmnExecutor::new(&model).evaluate(
        "result",
        HashMap::from([
            ("EstimatedMonthlyCost".into(), FeelValue::Number(250.0)),
            ("Region".into(), FeelValue::String("us".into())),
        ]),
    );
    assert_eq!(result.unwrap().value, FeelValue::Number(2.5));
}

#[test]
fn dmn15_formal_parameter_types_are_preserved_and_enforced() {
    let xml = format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}">
            <businessKnowledgeModel id="increment" name="increment">
                <variable name="increment" typeRef="number"/>
                <encapsulatedLogic kind="FEEL" typeRef="number">
                    <formalParameter name="x" typeRef="number"/>
                    <literalExpression><text>x + 1</text></literalExpression>
                </encapsulatedLogic>
            </businessKnowledgeModel>
            <decision id="valid" name="valid">
                <knowledgeRequirement><requiredKnowledge href="#increment"/></knowledgeRequirement>
                <invocation><literalExpression><text>increment</text></literalExpression><binding><parameter name="x"/><literalExpression><text>41</text></literalExpression></binding></invocation>
            </decision>
            <decision id="wrong-type" name="wrong type">
                <knowledgeRequirement><requiredKnowledge href="#increment"/></knowledgeRequirement>
                <invocation><literalExpression><text>increment</text></literalExpression><binding><parameter name="x"/><literalExpression><text>"41"</text></literalExpression></binding></invocation>
            </decision>
            <decision id="missing" name="missing">
                <knowledgeRequirement><requiredKnowledge href="#increment"/></knowledgeRequirement>
                <invocation><literalExpression><text>increment</text></literalExpression></invocation>
            </decision>
            <decision id="extra" name="extra">
                <knowledgeRequirement><requiredKnowledge href="#increment"/></knowledgeRequirement>
                <invocation><literalExpression><text>increment</text></literalExpression><binding><parameter name="x"/><literalExpression><text>1</text></literalExpression></binding><binding><parameter name="unused"/><literalExpression><text>2</text></literalExpression></binding></invocation>
            </decision>
        </definitions>"##
    );
    let model = parse_dmn(&xml).unwrap();
    let DecisionLogic::FunctionDefinition(function) = &model.business_knowledge_models[0].logic
    else {
        panic!("expected function definition");
    };
    assert_eq!(function.kind.as_deref(), Some("FEEL"));
    assert_eq!(function.parameter_type_refs, vec![Some("number".into())]);
    assert_eq!(function.result_type_ref.as_deref(), Some("number"));
    assert_eq!(
        model.business_knowledge_models[0].type_ref.as_deref(),
        Some("number")
    );
    assert_eq!(
        DmnExecutor::new(&model)
            .evaluate("valid", HashMap::new())
            .unwrap()
            .value,
        FeelValue::Number(42.0)
    );

    let wrong_type = DmnExecutor::new(&model).evaluate_by_id("wrong-type", HashMap::new());
    assert!(matches!(
        wrong_type.first_error().and_then(|message| message.error.as_ref()),
        Some(DmnError::TypeConstraintViolation { location, type_ref, .. })
            if location == "increment.x" && type_ref == "number"
    ));
    assert!(matches!(
        DmnExecutor::new(&model).evaluate(
            "missing",
            HashMap::from([("x".into(), FeelValue::Number(41.0))])
        ),
        Err(DmnError::Feel { detail, .. }) if detail.contains("missing invocation binding 'x'")
    ));
    assert!(matches!(
        DmnExecutor::new(&model).evaluate("extra", HashMap::new()),
        Err(DmnError::Feel { detail, .. }) if detail.contains("unknown invocation binding 'unused'")
    ));

    let serialized = model.to_dmn_xml().unwrap();
    assert!(serialized.contains("<variable name=\"increment\" typeRef=\"number\"/>"));
    assert!(serialized.contains("<encapsulatedLogic kind=\"FEEL\" typeRef=\"number\">"));
    assert!(serialized.contains("<formalParameter name=\"x\" typeRef=\"number\"/>"));
    let restored = parse_dmn(&serialized).unwrap();
    assert_eq!(
        DmnExecutor::new(&restored)
            .evaluate("valid", HashMap::new())
            .unwrap()
            .value,
        FeelValue::Number(42.0)
    );
}

#[test]
fn generated_feel_validation_rejects_unknown_bkm_invocation_binding() {
    let model = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}">
            <businessKnowledgeModel id="cost" name="BKM_CalculateCostRatio">
                <encapsulatedLogic>
                    <formalParameter name="monthlyCost" typeRef="number"/>
                    <literalExpression><text>monthlyCost / 100</text></literalExpression>
                </encapsulatedLogic>
            </businessKnowledgeModel>
            <decision id="ratio" name="CostRatio">
                <knowledgeRequirement><requiredKnowledge href="#cost"/></knowledgeRequirement>
                <invocation>
                    <literalExpression><text>BKM_CalculateCostRatio</text></literalExpression>
                    <binding><parameter name="estimatedMonthlyCost"/><literalExpression><text>100</text></literalExpression></binding>
                </invocation>
            </decision>
        </definitions>"##
    ))
    .unwrap();

    let error = validate_generated_feel_name_bindings(&model)
        .expect_err("unknown BKM invocation bindings must be rejected before submission");
    assert!(
        error
            .to_string()
            .contains("unknown invocation binding 'estimatedMonthlyCost'")
    );
    assert!(error.to_string().contains("BKM_CalculateCostRatio"));
}

#[test]
fn legacy_nested_bkm_function_definition_is_unwrapped_before_result_type_check() {
    let model = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}">
            <inputData id="amount-input" name="amount"><variable name="amount" typeRef="number"/></inputData>
            <businessKnowledgeModel id="cost" name="BKM_CalculateCostRatio">
                <variable name="BKM_CalculateCostRatio" typeRef="number"/>
                <encapsulatedLogic typeRef="number">
                    <formalParameter name="monthlyCost" typeRef="number"/>
                    <functionDefinition>
                        <formalParameter name="monthlyCost" typeRef="number"/>
                        <literalExpression><text>monthlyCost / 100</text></literalExpression>
                    </functionDefinition>
                </encapsulatedLogic>
            </businessKnowledgeModel>
            <decision id="ratio" name="CostRatio">
                <informationRequirement><requiredInput href="#amount-input"/></informationRequirement>
                <knowledgeRequirement><requiredKnowledge href="#cost"/></knowledgeRequirement>
                <variable name="CostRatio" typeRef="number"/>
                <invocation>
                    <literalExpression><text>BKM_CalculateCostRatio</text></literalExpression>
                    <binding><parameter name="monthlyCost"/><literalExpression><text>amount</text></literalExpression></binding>
                </invocation>
            </decision>
        </definitions>"##
    ))
    .unwrap();
    let result = DmnExecutor::new(&model).evaluate_by_name(
        "CostRatio",
        HashMap::from([("amount".into(), FeelValue::Number(250.0))]),
    );
    assert!(!result.has_errors(), "{:?}", result.messages);
    assert_eq!(
        result.decision_by_name("CostRatio").unwrap().value,
        FeelValue::Number(2.5)
    );
}

#[test]
fn dmn15_function_kinds_and_parameter_type_references_are_validated() {
    let external = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}"><businessKnowledgeModel id="external" name="external"><encapsulatedLogic kind="PMML"><context><contextEntry><variable name="document"/><literalExpression><text>"model.pmml"</text></literalExpression></contextEntry></context></encapsulatedLogic></businessKnowledgeModel></definitions>"##
    ))
    .unwrap();
    assert!(matches!(
        compile_dmn(&external),
        Err(DmnError::UnsupportedFeature { feature, .. }) if feature.contains("PMML")
    ));

    let unknown_type = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}"><businessKnowledgeModel id="typed" name="typed"><encapsulatedLogic kind="FEEL"><formalParameter name="value" typeRef="MissingType"/><literalExpression><text>value</text></literalExpression></encapsulatedLogic></businessKnowledgeModel></definitions>"##
    ))
    .unwrap();
    assert!(matches!(
        compile_dmn(&unknown_type),
        Err(DmnError::UnknownTypeReference { type_ref, .. }) if type_ref == "MissingType"
    ));
}

#[test]
fn dmn15_context_and_bkm_result_types_are_preserved_and_enforced() {
    let xml = format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}">
            <businessKnowledgeModel id="bad-return" name="badReturn">
                <variable name="badReturn" typeRef="number"/>
                <encapsulatedLogic kind="FEEL" typeRef="number"><literalExpression><text>"not a number"</text></literalExpression></encapsulatedLogic>
            </businessKnowledgeModel>
            <decision id="valid-context" name="valid context">
                <context>
                    <contextEntry><variable name="amount" typeRef="number"/><literalExpression><text>42</text></literalExpression></contextEntry>
                </context>
            </decision>
            <decision id="wrong-context" name="wrong context">
                <context>
                    <contextEntry><variable name="amount" typeRef="number"/><literalExpression><text>"42"</text></literalExpression></contextEntry>
                </context>
            </decision>
            <decision id="wrong-return" name="wrong return">
                <knowledgeRequirement><requiredKnowledge href="#bad-return"/></knowledgeRequirement>
                <invocation><literalExpression><text>badReturn</text></literalExpression></invocation>
            </decision>
        </definitions>"##
    );
    let model = parse_dmn(&xml).unwrap();
    let DecisionLogic::Context(entries) = &model.decisions[0].logic else {
        panic!("expected context");
    };
    assert_eq!(entries[0].type_ref.as_deref(), Some("number"));
    assert_eq!(
        DmnExecutor::new(&model)
            .evaluate("valid-context", HashMap::new())
            .unwrap()
            .value,
        FeelValue::Context(HashMap::from([("amount".into(), FeelValue::Number(42.0),)]))
    );

    for (decision, location) in [
        ("wrong-context", "wrong context.amount"),
        ("wrong-return", "badReturn.result"),
    ] {
        let result = DmnExecutor::new(&model).evaluate_by_id(decision, HashMap::new());
        assert!(matches!(
            result.first_error().and_then(|message| message.error.as_ref()),
            Some(DmnError::TypeConstraintViolation { location: actual, type_ref, .. })
                if actual == location && type_ref == "number"
        ));
    }

    let serialized = model.to_dmn_xml().unwrap();
    assert!(serialized.contains("<variable name=\"amount\" typeRef=\"number\"/>"));
    let restored = parse_dmn(&serialized).unwrap();
    let DecisionLogic::Context(entries) = &restored.decisions[0].logic else {
        panic!("expected restored context");
    };
    assert_eq!(entries[0].type_ref.as_deref(), Some("number"));
}

#[test]
fn dmn15_nested_expression_type_references_are_compiled() {
    let context = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}"><decision id="bad" name="bad"><context><contextEntry><variable name="value" typeRef="MissingType"/><literalExpression><text>1</text></literalExpression></contextEntry></context></decision></definitions>"##
    ))
    .unwrap();
    assert!(matches!(
        compile_dmn(&context),
        Err(DmnError::UnknownTypeReference { type_ref, .. }) if type_ref == "MissingType"
    ));

    let bkm = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}"><businessKnowledgeModel id="bad" name="bad"><variable name="bad" typeRef="MissingType"/><encapsulatedLogic><literalExpression><text>1</text></literalExpression></encapsulatedLogic></businessKnowledgeModel></definitions>"##
    ))
    .unwrap();
    assert!(matches!(
        compile_dmn(&bkm),
        Err(DmnError::UnknownTypeReference { type_ref, .. }) if type_ref == "MissingType"
    ));
}

#[test]
fn dmn15_feel_language_declarations_are_preserved_and_non_feel_is_rejected() {
    let feel_uri = "https://www.omg.org/spec/DMN/20230324/FEEL/";
    let model = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}" name="languages" expressionLanguage="{feel_uri}" typeLanguage="{feel_uri}"><decision id="result" name="result"><literalExpression expressionLanguage="FEEL"><text>42</text></literalExpression></decision></definitions>"##
    ))
    .unwrap();
    assert_eq!(model.expression_language.as_deref(), Some(feel_uri));
    assert_eq!(model.type_language.as_deref(), Some(feel_uri));
    compile_dmn(&model).unwrap();
    let serialized = model.to_dmn_xml().unwrap();
    assert!(serialized.contains(&format!("expressionLanguage=\"{feel_uri}\"")));
    assert!(serialized.contains(&format!("typeLanguage=\"{feel_uri}\"")));
    compile_dmn(&parse_dmn(&serialized).unwrap()).unwrap();

    let unsupported_root = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}" name="javascript" expressionLanguage="https://example.com/javascript"><decision id="result" name="result"><literalExpression><text>42</text></literalExpression></decision></definitions>"##
    ))
    .unwrap();
    assert!(matches!(
        compile_dmn(&unsupported_root),
        Err(DmnError::UnsupportedFeature { feature, .. })
            if feature.contains("expression language") && feature.contains("javascript")
    ));

    assert!(matches!(
        parse_dmn(&format!(
            r##"<definitions xmlns="{DMN_NAMESPACE}"><decision id="result" name="result"><literalExpression expressionLanguage="javascript"><text>42</text></literalExpression></decision></definitions>"##
        )),
        Err(DmnError::UnsupportedFeature { feature, .. }) if feature.contains("javascript")
    ));
    assert!(matches!(
        parse_dmn(&format!(
            r##"<definitions xmlns="{DMN_NAMESPACE}"><decision id="result" name="result"><decisionTable><input><inputExpression expressionLanguage="javascript"><text>1</text></inputExpression></input><output/><rule><inputEntry><text>-</text></inputEntry><outputEntry><text>1</text></outputEntry></rule></decisionTable></decision></definitions>"##
        )),
        Err(DmnError::UnsupportedFeature { feature, .. }) if feature.contains("javascript")
    ));
}

#[test]
fn dmn15_builtin_type_aliases_compile_and_validate_runtime_values() {
    let xml = format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}">
            <inputData id="text-input" name="text"><variable name="text" typeRef="feel.string"/></inputData>
            <inputData id="duration-input" name="period"><variable name="period" typeRef="yearMonthDuration"/></inputData>
            <decision id="text-result" name="text result"><variable name="text result" typeRef="feel:string"/><informationRequirement><requiredInput href="#text-input"/></informationRequirement><literalExpression><text>text</text></literalExpression></decision>
            <decision id="duration-result" name="duration result"><variable name="duration result" typeRef="years and months duration"/><informationRequirement><requiredInput href="#duration-input"/></informationRequirement><literalExpression><text>period</text></literalExpression></decision>
            <decision id="instance-result" name="instance result"><informationRequirement><requiredInput href="#duration-input"/></informationRequirement><literalExpression><text>period instance of yearMonthDuration</text></literalExpression></decision>
        </definitions>"##
    );
    let model = parse_dmn(&xml).unwrap();
    let inputs = HashMap::from([
        ("text".into(), FeelValue::String("hello".into())),
        ("period".into(), FeelValue::Duration("P2Y".into())),
    ]);
    let result = DmnExecutor::new(&model).evaluate_all(inputs);
    assert!(!result.has_errors(), "{:?}", result.messages);
    assert_eq!(
        result.decision_by_id("text-result").unwrap().value,
        FeelValue::String("hello".into())
    );
    assert_eq!(
        result.decision_by_id("duration-result").unwrap().value,
        FeelValue::Duration("P2Y".into())
    );
    assert_eq!(
        result.decision_by_id("instance-result").unwrap().value,
        FeelValue::Bool(true)
    );
}

#[test]
fn dmn15_context_function_definitions_are_invocable_and_round_trip() {
    let xml = format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}"><decision id="result" name="result"><context>
            <contextEntry><variable name="add" typeRef="function"/><functionDefinition kind="FEEL" typeRef="number"><formalParameter name="left" typeRef="number"/><formalParameter name="right" typeRef="number"/><literalExpression><text>left + right</text></literalExpression></functionDefinition></contextEntry>
            <contextEntry><invocation><literalExpression><text>add</text></literalExpression><binding><parameter name="left"/><literalExpression><text>2</text></literalExpression></binding><binding><parameter name="right"/><literalExpression><text>3</text></literalExpression></binding></invocation></contextEntry>
        </context></decision></definitions>"##
    );
    let model = parse_dmn(&xml).unwrap();
    for model in [&model, &parse_dmn(&model.to_dmn_xml().unwrap()).unwrap()] {
        assert_eq!(
            DmnExecutor::new(model)
                .evaluate("result", HashMap::new())
                .unwrap()
                .value,
            FeelValue::Number(5.0)
        );
    }

    let missing = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}"><decision id="result" name="result"><context><contextEntry><variable name="add" typeRef="function"/><functionDefinition><formalParameter name="left"/><formalParameter name="right"/><literalExpression><text>left + right</text></literalExpression></functionDefinition></contextEntry><contextEntry><invocation><literalExpression><text>add</text></literalExpression><binding><parameter name="left"/><literalExpression><text>2</text></literalExpression></binding></invocation></contextEntry></context></decision></definitions>"##
    ))
    .unwrap();
    assert!(matches!(
        DmnExecutor::new(&missing).evaluate("result", HashMap::new()),
        Err(DmnError::Feel { detail, .. }) if detail.contains("missing invocation binding 'right'")
    ));
}

#[test]
fn dmn15_empty_literal_and_default_outputs_evaluate_to_null() {
    let literal = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}"><decision id="result" name="result"><literalExpression><text/></literalExpression></decision></definitions>"##
    ))
    .unwrap();
    assert_eq!(
        DmnExecutor::new(&literal)
            .evaluate("result", HashMap::new())
            .unwrap()
            .value,
        FeelValue::Null
    );

    let table = model(
        r#"<decisionTable><input><inputExpression><text>age</text></inputExpression></input><output name="result"><defaultOutputEntry><text/></defaultOutputEntry></output><rule><inputEntry><text>&gt; 18</text></inputEntry><outputEntry><text>1</text></outputEntry></rule></decisionTable>"#,
    );
    assert_eq!(
        DmnExecutor::new(&table)
            .evaluate(
                "result",
                HashMap::from([("age".into(), FeelValue::Number(10.0))])
            )
            .unwrap()
            .value,
        FeelValue::Null
    );
}

#[test]
fn dmn15_function_definitions_accept_non_literal_boxed_bodies() {
    let model = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}"><decision id="result" name="result"><context><contextEntry><variable name="build" typeRef="function"/><functionDefinition><formalParameter name="value" typeRef="number"/><context><contextEntry><variable name="original" typeRef="number"/><literalExpression><text>value</text></literalExpression></contextEntry><contextEntry><literalExpression><text>{{original: original}}</text></literalExpression></contextEntry></context></functionDefinition></contextEntry></context></decision></definitions>"##
    ))
    .unwrap();
    let DecisionLogic::Context(entries) = &model.decisions[0].logic else {
        panic!("expected outer context");
    };
    let DecisionLogic::FunctionDefinition(function) = &entries[0].expression else {
        panic!("expected function definition");
    };
    assert!(matches!(function.body.as_ref(), DecisionLogic::Context(_)));
    compile_dmn(&model).unwrap();
    let restored = parse_dmn(&model.to_dmn_xml().unwrap()).unwrap();
    let DecisionLogic::Context(entries) = &restored.decisions[0].logic else {
        panic!("expected restored outer context");
    };
    let DecisionLogic::FunctionDefinition(function) = &entries[0].expression else {
        panic!("expected restored function definition");
    };
    assert!(matches!(function.body.as_ref(), DecisionLogic::Context(_)));
}

#[test]
fn legacy_equals_prefixed_unary_tests_are_supported() {
    let table = model(
        r#"<decisionTable hitPolicy="FIRST"><input><inputExpression><text>age</text></inputExpression></input><output name="result"/><rule><inputEntry><text>= 18</text></inputEntry><outputEntry><text>"equal"</text></outputEntry></rule><rule><inputEntry><text>-</text></inputEntry><outputEntry><text>"other"</text></outputEntry></rule></decisionTable>"#,
    );
    assert_eq!(
        DmnExecutor::new(&table)
            .evaluate(
                "result",
                HashMap::from([("age".into(), FeelValue::Number(18.0))])
            )
            .unwrap()
            .value,
        FeelValue::String("equal".into())
    );
}

#[test]
fn feel_any_type_accepts_values_after_namespace_prefix_normalization() {
    let model = parse_dmn(&format!(
        r##"<definitions xmlns="{DMN_NAMESPACE}"><inputData id="score-input" name="score"><variable name="score" typeRef="feel:number"/></inputData><decision id="rating" name="rating"><informationRequirement><requiredInput href="#score-input"/></informationRequirement><decisionTable hitPolicy="FIRST"><input><inputExpression typeRef="feel:Any"><text>score</text></inputExpression></input><output name="result" typeRef="feel:string"/><rule><inputEntry><text>&gt;= 700</text></inputEntry><outputEntry><text>"high"</text></outputEntry></rule><rule><inputEntry><text>-</text></inputEntry><outputEntry><text>"other"</text></outputEntry></rule></decisionTable></decision></definitions>"##
    ))
    .unwrap();
    let result = DmnExecutor::new(&model)
        .evaluate(
            "rating",
            HashMap::from([("score".into(), FeelValue::Number(720.0))]),
        )
        .unwrap();
    assert_eq!(result.value, FeelValue::String("high".into()));
}

#[test]
fn loan_fixture_executes_when_decision_table_inputs_use_feel_any() {
    let model = parse_dmn(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/resource/loan_0812.dmn"
    )))
    .unwrap();
    let result = DmnExecutor::new(&model).evaluate_all(HashMap::from([
        ("ApplicantAge".into(), FeelValue::Number(35.0)),
        ("ApplicantIncome".into(), FeelValue::Number(80_000.0)),
        ("CreditScore".into(), FeelValue::Number(760.0)),
    ]));
    assert!(!result.has_errors(), "{:?}", result.messages);
    assert_eq!(
        result.decision_by_name("LoanApproval").unwrap().value,
        FeelValue::String("Approve".into())
    );
}

#[test]
fn test_dinner_dmn() {
    let model = parse_dmn(DINNER_F).unwrap();
    let result = DmnExecutor::new(&model).evaluate_all(HashMap::from([
        ("Guests with children".into(), FeelValue::Bool(true)),
        ("Season".into(), FeelValue::String("Fall".into())),
        ("Number of guests".into(), FeelValue::Number(4.0)),
        ("Temp".into(), FeelValue::Number(25.0)),
        ("Rain Probability".into(), FeelValue::Number(30.0)),
    ]));
    assert!(!result.has_errors(), "{:?}", result.messages);
    assert_eq!(
        result.decision_by_name("Dish").unwrap().value,
        FeelValue::String("Spareribs".into())
    );
    assert_eq!(
        result.decision_by_name("Where to eat").unwrap().value,
        FeelValue::String("Outside".into())
    );
    assert_eq!(
        result.decision_by_name("Drinks").unwrap().value,
        FeelValue::List(vec![
            FeelValue::String("Apero".into()),
            FeelValue::String("Ale".into()),
            FeelValue::String("Juice Boxes".into()),
        ])
    );
}
