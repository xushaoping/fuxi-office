use gloo_storage::Storage;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde_yaml::Value as YamlValue;
use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::JsFuture;

use crate::api;
use crate::components::auth::auth_token;
use crate::components::workspace::{
    CopyStatus, YAML_GRAPH_NODE_HEIGHT, YAML_GRAPH_NODE_WIDTH, YAML_GRAPH_WIDTH, YamlGraphEdge,
    YamlGraphNode, highlight_code, indexed, yaml_get, yaml_graph_edge_anchors,
    yaml_graph_edge_path, yaml_graph_height, yaml_graph_knowledge_points, yaml_graph_label,
    yaml_joined_list, yaml_list, yaml_map, yaml_text,
};

#[component]
fn Artifact(
    label: &'static str,
    language: &'static str,
    value: ReadSignal<Option<String>>,
    set_value: Option<WriteSignal<Option<String>>>,
    set_xml: Option<WriteSignal<Option<String>>>,
    set_source: Option<WriteSignal<String>>,
) -> impl IntoView {
    if language == "yaml" {
        return view! { <YamlArtifact value=value set_value=set_value set_xml=set_xml set_source=set_source /> }
            .into_any();
    }

    let copy_status = RwSignal::new(CopyStatus::Idle);
    Effect::new(move |_| {
        let _ = value.get();
        copy_status.set(CopyStatus::Idle);
    });
    let copy = move |_| {
        let Some(content) = value.get() else {
            return;
        };
        copy_status.set(CopyStatus::Idle);
        spawn_local(async move {
            let result = if let Some(window) = web_sys::window() {
                JsFuture::from(window.navigator().clipboard().write_text(&content))
                    .await
                    .map_err(|_| ())
            } else {
                Err(())
            };
            copy_status.set(if result.is_ok() {
                CopyStatus::Copied
            } else {
                CopyStatus::Failed
            });
        });
    };

    view! {
        <div class="artifact-panel">
            <div class="artifact-heading">
                <strong>{label}</strong>
                <button
                    class="button button-quiet"
                    on:click=copy
                    disabled=move || value.get().is_none()
                >
                    {move || match copy_status.get() {
                        CopyStatus::Idle => "复制",
                        CopyStatus::Copied => "已复制",
                        CopyStatus::Failed => "复制失败",
                    }}
                </button>
            </div>
            <pre class=format!("code-view code-view-{}", language)>
                <code inner_html=move || {
                    value.get()
                        .map(|content| highlight_code(&content, language))
                        .unwrap_or_else(|| "等待生成...".into())
                }>
                </code>
            </pre>
        </div>
    }
    .into_any()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ArtifactTab {
    Yaml,
    Xml,
}

#[component]
pub(crate) fn ArtifactTabs(
    dsl: ReadSignal<Option<String>>,
    set_dsl: WriteSignal<Option<String>>,
    xml: ReadSignal<Option<String>>,
    set_xml: WriteSignal<Option<String>>,
    set_source: WriteSignal<String>,
) -> impl IntoView {
    let active = RwSignal::new(ArtifactTab::Yaml);
    view! {
        <div class="artifact-workbench">
            <div class="artifact-tabs" role="tablist" aria-label="模型产物">
                <button
                    class=move || if active.get() == ArtifactTab::Yaml { "artifact-tab is-active" } else { "artifact-tab" }
                    on:click=move |_| active.set(ArtifactTab::Yaml)
                    role="tab"
                    aria-selected=move || active.get() == ArtifactTab::Yaml
                >"DSL YAML"</button>
                <button
                    class=move || if active.get() == ArtifactTab::Xml { "artifact-tab is-active" } else { "artifact-tab" }
                    on:click=move |_| active.set(ArtifactTab::Xml)
                    role="tab"
                    aria-selected=move || active.get() == ArtifactTab::Xml
                >"DMN XML"</button>
            </div>
            <div class="artifact-tab-content">
                <Show when=move || active.get() == ArtifactTab::Yaml>
                    <YamlArtifact value=dsl set_value=Some(set_dsl) set_xml=Some(set_xml) set_source=Some(set_source) />
                </Show>
                <Show when=move || active.get() == ArtifactTab::Xml>
                    <Artifact label="DMN XML" language="xml" value=xml set_value=None set_xml=None set_source=None />
                </Show>
            </div>
        </div>
    }
}
#[derive(Clone, PartialEq, Eq, Default)]
struct YamlSummary {
    name: String,
    namespace: String,
    description: String,
    item_definitions: usize,
    inputs: usize,
    input_details: Vec<YamlField>,
    type_details: Vec<YamlField>,
    decisions: Vec<YamlDecision>,
    graph_nodes: Vec<YamlGraphNode>,
    graph_edges: Vec<YamlGraphEdge>,
    bkms: Vec<YamlKnowledgeModel>,
    decision_services: Vec<YamlDecisionService>,
    knowledge_sources: usize,
    text_annotations: usize,
    raw: Option<YamlValue>,
    error: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Default)]
struct YamlDecision {
    name: String,
    element_type: String,
    description: String,
    type_ref: String,
    hit_policy: String,
    inputs: Vec<String>,
    outputs: Vec<String>,
    rules: Vec<YamlRule>,
    required_inputs: Vec<String>,
    required_decisions: Vec<String>,
    required_knowledge: Vec<String>,
    expression_type: String,
    expression_summary: String,
}

#[derive(Clone, PartialEq, Eq, Default)]
struct YamlKnowledgeModel {
    name: String,
    element_type: String,
    description: String,
    type_ref: String,
    parameters: Vec<String>,
    expression_type: String,
    expression_summary: String,
}

#[derive(Clone, PartialEq, Eq, Default)]
struct YamlDecisionService {
    name: String,
    element_type: String,
    description: String,
    input_data: Vec<String>,
    encapsulated_decisions: Vec<String>,
    output_decisions: Vec<String>,
    expression_summary: String,
}
#[derive(Clone, PartialEq, Eq, Default)]
struct YamlRule {
    inputs: Vec<String>,
    outputs: Vec<String>,
    annotation: String,
}

#[derive(Clone, PartialEq, Eq, Default)]
struct YamlField {
    name: String,
    element_type: String,
    type_ref: String,
    description: String,
}
/// Builds a compact, readable preview for every boxed-expression variant.
/// Invocation and function expressions do not have a top-level `expression`
/// field, so falling back to a generic placeholder made those definitions
/// appear empty in the model panel.
fn yaml_expression_preview(logic: &serde_yaml::Mapping) -> String {
    let expression_type = yaml_text(yaml_get(logic, "type"));
    match expression_type.as_str() {
        "literalExpression" => ["expression", "text", "value", "body", "literal"]
            .iter()
            .find_map(|key| yaml_get(logic, key).map(|value| yaml_text(Some(value))))
            .unwrap_or_default(),
        "invocation" => {
            let bkm = yaml_text(yaml_get(logic, "bkm"));
            let bindings = yaml_list(yaml_get(logic, "bindings"))
                .iter()
                .filter_map(|value| {
                    let binding = yaml_map(value)?;
                    let parameter = yaml_text(yaml_get(binding, "parameter"));
                    let expression = yaml_text(yaml_get(binding, "expression"));
                    Some(if parameter.is_empty() {
                        expression
                    } else if expression.is_empty() {
                        parameter
                    } else {
                        format!("{parameter} = {expression}")
                    })
                })
                .filter(|binding| !binding.is_empty())
                .collect::<Vec<_>>();
            if bindings.is_empty() {
                format!("{bkm}()")
            } else {
                format!("{bkm}({})", bindings.join(", "))
            }
        }
        "functionDefinition" => {
            let parameters = yaml_list(yaml_get(logic, "parameters"))
                .iter()
                .filter_map(|value| {
                    let parameter = yaml_map(value)?;
                    let name = yaml_text(yaml_get(parameter, "name"));
                    let type_ref = yaml_text(yaml_get(parameter, "typeRef"));
                    Some(if type_ref.is_empty() {
                        name
                    } else {
                        format!("{name}: {type_ref}")
                    })
                })
                .filter(|parameter| !parameter.is_empty())
                .collect::<Vec<_>>();
            let body = yaml_get(logic, "body")
                .and_then(yaml_map)
                .map(yaml_expression_preview)
                .unwrap_or_default();
            format!(
                "function ({}){}",
                parameters.join(", "),
                if body.is_empty() {
                    String::new()
                } else {
                    format!(" {{ {body} }}")
                }
            )
        }
        "context" => {
            let entries = yaml_list(yaml_get(logic, "entries"))
                .iter()
                .filter_map(|value| {
                    let entry = yaml_map(value)?;
                    let key = yaml_text(yaml_get(entry, "key"));
                    let value = yaml_get(entry, "value")
                        .and_then(yaml_map)
                        .map(yaml_expression_preview)
                        .unwrap_or_else(|| yaml_text(yaml_get(entry, "value")));
                    Some(if value.is_empty() {
                        key
                    } else {
                        format!("{key}: {value}")
                    })
                })
                .filter(|entry| !entry.is_empty())
                .collect::<Vec<_>>();
            format!("{{ {} }}", entries.join(", "))
        }
        "list" => format!("[{}]", yaml_joined_list(yaml_get(logic, "items"))),
        "relation" => {
            let columns = yaml_joined_list(yaml_get(logic, "columns"));
            let rows = yaml_list(yaml_get(logic, "rows")).len();
            format!("relation ({columns}) · {rows} 行")
        }
        "conditional" => {
            let condition = yaml_text(yaml_get(logic, "condition"));
            let then = yaml_get(logic, "then")
                .and_then(yaml_map)
                .map(yaml_expression_preview)
                .unwrap_or_default();
            let else_ = yaml_get(logic, "else")
                .and_then(yaml_map)
                .map(yaml_expression_preview)
                .unwrap_or_default();
            format!("if {condition} then {then} else {else_}")
        }
        "filter" => format!(
            "{}[{}]",
            yaml_text(yaml_get(logic, "source")),
            yaml_text(yaml_get(logic, "filter"))
        ),
        "for" => format!(
            "for {} in {} return {}",
            yaml_text(yaml_get(logic, "variable")),
            yaml_text(yaml_get(logic, "in")),
            yaml_text(yaml_get(logic, "return"))
        ),
        "quantified" => format!(
            "{} {} in {} satisfies {}",
            yaml_text(yaml_get(logic, "kind")),
            yaml_text(yaml_get(logic, "variable")),
            yaml_text(yaml_get(logic, "in")),
            yaml_text(yaml_get(logic, "satisfies"))
        ),
        "decisionTable" => {
            let inputs = yaml_list(yaml_get(logic, "inputs")).len();
            let outputs = yaml_list(yaml_get(logic, "outputs")).len();
            let rules = yaml_list(yaml_get(logic, "rules")).len();
            format!("决策表 · {inputs} 个输入 · {outputs} 个输出 · {rules} 条规则")
        }
        _ => serde_yaml::to_string(logic)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}

pub(crate) fn yaml_summary(source: &str) -> YamlSummary {
    let raw = match serde_yaml::from_str::<YamlValue>(source) {
        Ok(value) => value,
        Err(error) => {
            return YamlSummary {
                error: Some(format!("YAML 解析失败：{error}")),
                ..Default::default()
            };
        }
    };
    let Some(root) = yaml_map(&raw) else {
        return YamlSummary {
            raw: Some(raw),
            error: Some("YAML 根节点必须是对象".into()),
            ..Default::default()
        };
    };
    let decisions = yaml_list(yaml_get(root, "decisions"))
        .iter()
        .filter_map(|value| {
            let map = yaml_map(value)?;
            let logic = yaml_get(map, "logic").and_then(yaml_map);
            let table = logic.filter(|logic| yaml_text(yaml_get(logic, "type")) == "decisionTable");
            let expression_type = logic
                .map(|logic| yaml_text(yaml_get(logic, "type")))
                .unwrap_or_else(|| "literalExpression".into());
            let expression_summary = logic.map(yaml_expression_preview).unwrap_or_default();
            let inputs = table
                .and_then(|table| yaml_get(table, "inputs"))
                .map(|value| yaml_list(Some(value)))
                .unwrap_or_default()
                .iter()
                .map(|value| {
                    yaml_map(value)
                        .map(|input| yaml_text(yaml_get(input, "name")))
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>();
            let outputs = table
                .and_then(|table| yaml_get(table, "outputs"))
                .map(|value| yaml_list(Some(value)))
                .unwrap_or_default()
                .iter()
                .map(|value| {
                    yaml_map(value)
                        .map(|output| yaml_text(yaml_get(output, "name")))
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>();
            let rules = table
                .and_then(|table| yaml_get(table, "rules"))
                .map(|value| yaml_list(Some(value)))
                .unwrap_or_default()
                .iter()
                .filter_map(|value| {
                    let rule = yaml_map(value)?;
                    Some(YamlRule {
                        inputs: yaml_list(yaml_get(rule, "inputEntries"))
                            .iter()
                            .map(|v| yaml_text(Some(v)))
                            .collect(),
                        outputs: yaml_list(yaml_get(rule, "outputEntries"))
                            .iter()
                            .map(|v| yaml_text(Some(v)))
                            .collect(),
                        annotation: yaml_text(yaml_get(rule, "annotation")),
                    })
                })
                .collect();
            Some(YamlDecision {
                name: yaml_text(yaml_get(map, "name")),
                element_type: "决策".into(),
                description: yaml_text(yaml_get(map, "description")),
                type_ref: yaml_text(yaml_get(map, "typeRef")),
                hit_policy: table
                    .map(|table| yaml_text(yaml_get(table, "hitPolicy")))
                    .unwrap_or_default(),
                inputs,
                outputs,
                rules,
                required_inputs: yaml_list(yaml_get(map, "requiredInputs"))
                    .iter()
                    .map(|value| yaml_text(Some(value)))
                    .collect(),
                required_decisions: yaml_list(yaml_get(map, "requiredDecisions"))
                    .iter()
                    .map(|value| yaml_text(Some(value)))
                    .collect(),
                required_knowledge: yaml_list(yaml_get(map, "requiredKnowledge"))
                    .iter()
                    .map(|value| yaml_text(Some(value)))
                    .collect(),
                expression_type,
                expression_summary,
            })
        })
        .collect::<Vec<_>>();
    let bkms = yaml_list(yaml_get(root, "bkms"))
        .iter()
        .filter_map(|value| {
            let map = yaml_map(value)?;
            let logic = yaml_get(map, "logic").and_then(yaml_map);
            Some(YamlKnowledgeModel {
                name: yaml_text(yaml_get(map, "name")),
                element_type: "业务知识模型".into(),
                description: yaml_text(yaml_get(map, "description")),
                type_ref: yaml_text(yaml_get(map, "typeRef")),
                parameters: yaml_list(yaml_get(map, "parameters"))
                    .iter()
                    .filter_map(|value| {
                        let parameter = yaml_map(value)?;
                        let name = yaml_text(yaml_get(parameter, "name"));
                        let type_ref = yaml_text(yaml_get(parameter, "typeRef"));
                        Some(if type_ref.is_empty() {
                            name
                        } else {
                            format!("{name}: {type_ref}")
                        })
                    })
                    .filter(|parameter| !parameter.is_empty())
                    .collect(),
                expression_type: logic
                    .map(|logic| yaml_text(yaml_get(logic, "type")))
                    .unwrap_or_else(|| "literalExpression".into()),
                expression_summary: logic.map(yaml_expression_preview).unwrap_or_default(),
            })
        })
        .filter(|bkm| !bkm.name.is_empty())
        .collect::<Vec<_>>();
    let decision_services = yaml_list(yaml_get(root, "decisionServices"))
        .iter()
        .filter_map(|value| {
            let map = yaml_map(value)?;
            let input_data = yaml_joined_list(yaml_get(map, "inputData"));
            let encapsulated_decisions = yaml_joined_list(yaml_get(map, "encapsulatedDecisions"));
            let output_decisions = yaml_joined_list(yaml_get(map, "outputDecisions"));
            let groups = [
                (!input_data.is_empty(), format!("输入: {input_data}")),
                (
                    !encapsulated_decisions.is_empty(),
                    format!("封装: {encapsulated_decisions}"),
                ),
                (
                    !output_decisions.is_empty(),
                    format!("输出: {output_decisions}"),
                ),
            ];
            Some(YamlDecisionService {
                name: yaml_text(yaml_get(map, "name")),
                element_type: "决策服务".into(),
                description: yaml_text(yaml_get(map, "description")),
                input_data: yaml_list(yaml_get(map, "inputData"))
                    .iter()
                    .map(|value| yaml_text(Some(value)))
                    .filter(|value| !value.is_empty())
                    .collect(),
                encapsulated_decisions: yaml_list(yaml_get(map, "encapsulatedDecisions"))
                    .iter()
                    .map(|value| yaml_text(Some(value)))
                    .filter(|value| !value.is_empty())
                    .collect(),
                output_decisions: yaml_list(yaml_get(map, "outputDecisions"))
                    .iter()
                    .map(|value| yaml_text(Some(value)))
                    .filter(|value| !value.is_empty())
                    .collect(),
                expression_summary: groups
                    .into_iter()
                    .filter_map(|(present, text)| present.then_some(text))
                    .collect::<Vec<_>>()
                    .join(" · "),
            })
        })
        .filter(|service| !service.name.is_empty())
        .collect::<Vec<_>>();
    let count = |key| yaml_list(yaml_get(root, key)).len();
    let input_names = yaml_list(yaml_get(root, "inputs"))
        .iter()
        .filter_map(|value| yaml_map(value).map(|map| yaml_text(yaml_get(map, "name"))))
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    let input_details = yaml_list(yaml_get(root, "inputs"))
        .iter()
        .filter_map(|value| {
            let map = yaml_map(value)?;
            Some(YamlField {
                name: yaml_text(yaml_get(map, "name")),
                element_type: "输入数据".into(),
                type_ref: yaml_text(yaml_get(map, "typeRef")),
                description: yaml_text(yaml_get(map, "description")),
            })
        })
        .filter(|field| !field.name.is_empty())
        .collect::<Vec<_>>();
    let type_details = yaml_list(yaml_get(root, "itemDefinitions"))
        .iter()
        .filter_map(|value| {
            let map = yaml_map(value)?;
            Some(YamlField {
                name: yaml_text(yaml_get(map, "name")),
                element_type: "数据类型".into(),
                type_ref: yaml_text(yaml_get(map, "type")),
                description: yaml_text(yaml_get(map, "description")),
            })
        })
        .filter(|field| !field.name.is_empty())
        .collect::<Vec<_>>();
    let bkm_names = yaml_list(yaml_get(root, "bkms"))
        .iter()
        .filter_map(|value| yaml_map(value).map(|map| yaml_text(yaml_get(map, "name"))))
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    let mut graph_nodes = Vec::new();
    for (index, name) in input_names.iter().enumerate() {
        graph_nodes.push(YamlGraphNode {
            name: name.clone(),
            kind: "input".into(),
            x: 34,
            y: 34 + index as i32 * 58,
        });
    }
    for (index, decision) in decisions.iter().enumerate() {
        graph_nodes.push(YamlGraphNode {
            name: decision.name.clone(),
            kind: "decision".into(),
            x: 322,
            y: 34 + index as i32 * 72,
        });
    }
    for (index, name) in bkm_names.iter().enumerate() {
        graph_nodes.push(YamlGraphNode {
            name: name.clone(),
            kind: "knowledge".into(),
            x: 610,
            y: 34 + index as i32 * 58,
        });
    }
    let position = |name: &str| graph_nodes.iter().find(|node| node.name == name).cloned();
    let mut graph_edges = Vec::new();
    for decision in &decisions {
        for source in decision
            .required_inputs
            .iter()
            .chain(decision.required_decisions.iter())
            .chain(decision.required_knowledge.iter())
        {
            if let (Some(source_node), Some(target_node)) =
                (position(source), position(&decision.name))
            {
                let (x1, y1, x2, y2) = yaml_graph_edge_anchors(&source_node, &target_node);
                graph_edges.push(YamlGraphEdge {
                    source: source.to_string(),
                    target: decision.name.clone(),
                    kind: source_node.kind,
                    x1,
                    y1,
                    x2,
                    y2,
                });
            }
        }
    }
    YamlSummary {
        name: yaml_text(yaml_get(root, "name")),
        namespace: yaml_text(yaml_get(root, "namespace")),
        description: yaml_text(yaml_get(root, "description")),
        item_definitions: count("itemDefinitions"),
        inputs: count("inputs"),
        input_details,
        type_details,
        decisions,
        graph_nodes,
        graph_edges,
        bkms,
        decision_services,
        knowledge_sources: count("knowledgeSources"),
        text_annotations: count("textAnnotations"),
        raw: Some(raw),
        error: None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum YamlView {
    Model,
    Dependency,
    Structure,
    Source,
}

#[component]
fn YamlArtifact(
    value: ReadSignal<Option<String>>,
    set_value: Option<WriteSignal<Option<String>>>,
    set_xml: Option<WriteSignal<Option<String>>>,
    set_source: Option<WriteSignal<String>>,
) -> impl IntoView {
    let view = RwSignal::new(YamlView::Model);
    let copy_status = RwSignal::new(CopyStatus::Idle);
    let source_text = RwSignal::new(value.get_untracked().unwrap_or_default());
    let source_error = RwSignal::new(None::<String>);
    let compile_state = RwSignal::new(None::<String>);
    let editing_source = RwSignal::new(false);
    Effect::new(move |_| {
        let next_value = value.get();
        source_text.set(next_value.clone().unwrap_or_default());
        if next_value.is_none() {
            source_error.set(None);
            compile_state.set(None);
            editing_source.set(false);
        }
    });
    let summary = Memo::new(move |_| {
        value
            .get()
            .map(|source| yaml_summary(&source))
            .unwrap_or_default()
    });
    let copy = move |_| {
        let Some(content) = value.get() else {
            return;
        };
        spawn_local(async move {
            let copied = if let Some(window) = web_sys::window() {
                JsFuture::from(window.navigator().clipboard().write_text(&content))
                    .await
                    .is_ok()
            } else {
                false
            };
            copy_status.set(if copied {
                CopyStatus::Copied
            } else {
                CopyStatus::Failed
            });
        });
    };
    view! {
        <div class="artifact-block yaml-artifact">
            <div class="artifact-heading">
                <span>"DSL YAML"</span>
                <div class="artifact-heading-actions">
                    <span class="artifact-type">"YAML"</span>
                    <button class="button button-quiet artifact-copy" on:click=copy title="复制 YAML">
                        {move || match copy_status.get() { CopyStatus::Idle => "复制", CopyStatus::Copied => "已复制", CopyStatus::Failed => "复制失败" }}
                    </button>
                </div>
            </div>
            <div class="yaml-view-tabs" role="tablist" aria-label="YAML 展示方式">
                <button class=move || if view.get() == YamlView::Model { "yaml-view-tab is-active" } else { "yaml-view-tab" } on:click=move |_| view.set(YamlView::Model)>"模型"</button>
                <button class=move || if view.get() == YamlView::Dependency { "yaml-view-tab is-active" } else { "yaml-view-tab" } on:click=move |_| view.set(YamlView::Dependency)>"决策依赖图"</button>
                <button class=move || if view.get() == YamlView::Structure { "yaml-view-tab is-active" } else { "yaml-view-tab" } on:click=move |_| view.set(YamlView::Structure)>"结构"</button>
                <button class=move || if view.get() == YamlView::Source { "yaml-view-tab is-active" } else { "yaml-view-tab" } on:click=move |_| view.set(YamlView::Source)>"源码"</button>
            </div>
            <Show when=move || compile_state.get().is_some()>
                <div class=move || if compile_state.get().as_deref() == Some("DMN XML 已同步") { "yaml-sync-status is-success" } else if compile_state.get().as_deref() == Some("YAML 已应用，XML 编译失败") { "yaml-sync-status is-error" } else { "yaml-sync-status" }>{move || compile_state.get().unwrap_or_default()}</div>
            </Show>
            <Show when=move || view.get() == YamlView::Model>
                <YamlModelView summary=summary />
            </Show>
            <Show when=move || view.get() == YamlView::Dependency>
                <YamlDependencyGraphView summary=summary />
            </Show>
            <Show when=move || view.get() == YamlView::Structure>
                <YamlStructureView summary=summary />
            </Show>
            <Show when=move || view.get() == YamlView::Source>
                <div class="yaml-view-content">
                    <Show
                        when=move || editing_source.get()
                        fallback=move || view! {
                            <div class="yaml-source-preview">
                                <pre class="code-view code-view-yaml yaml-source-view"><code inner_html=move || highlight_code(&value.get().unwrap_or_else(|| "等待生成...".into()), "yaml")></code></pre>
                                <div class="yaml-source-actions"><button class="button button-quiet" on:click=move |_| editing_source.set(true) disabled=move || set_value.is_none()>"编辑 YAML"</button></div>
                            </div>
                        }
                    >
                        <div class="yaml-source-editor">
                            <textarea class="yaml-source-input" prop:value=source_text on:input=move |event| { source_text.set(event_target_value(&event)); source_error.set(None); } placeholder="编辑 DSL YAML..."></textarea>
                            <div class="yaml-source-actions">
                                <Show when=move || source_error.get().is_some()><span class="yaml-source-error">{move || source_error.get().unwrap_or_default()}</span></Show>
                                <button class="button button-quiet" on:click=move |_| { source_text.set(value.get_untracked().unwrap_or_default()); source_error.set(None); editing_source.set(false); } >"取消"</button>
                                <button class="button button-primary" on:click=move |_| {
                                    let text = source_text.get();
                                    match serde_yaml::from_str::<YamlValue>(&text) {
                                        Ok(parsed) if yaml_map(&parsed).is_some() => {
                                            if let Some(set_value) = set_value { set_value.set(Some(text.clone())); }
                                            if let Some(set_source) = set_source { set_source.set(text.clone()); }
                                            compile_state.set(Some("正在编译 DMN XML...".into()));
                                            source_error.set(None);
                                            editing_source.set(false);
                                            if let (Some(set_xml), Some(token)) = (set_xml, auth_token().get_untracked()) {
                                                let dsl_text = source_text.get();
                                                spawn_local(async move {
                                                    match api::compile_dsl(&token, dsl_text).await {
                                                        Ok(xml) => { set_xml.set(Some(xml)); compile_state.set(Some("DMN XML 已同步".into())); }
                                                        Err(error) => { compile_state.set(Some("YAML 已应用，XML 编译失败".into())); source_error.set(Some(error)); }
                                                    }
                                                });
                                            } else {
                                                compile_state.set(Some("YAML 已应用".into()));
                                            }
                                            view.set(YamlView::Model);
                                        }
                                        Ok(_) => source_error.set(Some("YAML 根节点必须是对象".into())),
                                        Err(error) => source_error.set(Some(format!("YAML 解析失败：{error}"))),
                                    }
                                } disabled=move || set_value.is_none()>"应用 YAML"</button>
                            </div>
                        </div>
                    </Show>
                </div>
            </Show>
        </div>
    }
}

fn yaml_kind(value: &YamlValue) -> &'static str {
    match value {
        YamlValue::Mapping(_) => "对象",
        YamlValue::Sequence(_) => "数组",
        YamlValue::String(_) => "字符串",
        YamlValue::Number(_) => "数字",
        YamlValue::Bool(_) => "布尔值",
        YamlValue::Null => "空值",
        YamlValue::Tagged(_) => "值",
    }
}

fn yaml_preview(value: &YamlValue) -> String {
    match value {
        YamlValue::Mapping(map) => format!("{} 个字段", map.len()),
        YamlValue::Sequence(items) => format!("{} 项", items.len()),
        _ => yaml_text(Some(value)),
    }
}

#[derive(Clone)]
struct YamlPathEntry {
    path: String,
    kind: String,
    preview: String,
    depth: usize,
}

fn collect_yaml_paths(
    value: &YamlValue,
    path: &str,
    depth: usize,
    output: &mut Vec<YamlPathEntry>,
) {
    if depth > 3 {
        return;
    }
    match value {
        YamlValue::Mapping(map) => {
            for (key, child) in map {
                let key = yaml_text(Some(key));
                let child_path = if path.is_empty() {
                    key
                } else {
                    format!("{path}.{key}")
                };
                output.push(YamlPathEntry {
                    path: child_path.clone(),
                    kind: yaml_kind(child).into(),
                    preview: yaml_preview(child),
                    depth: depth + 1,
                });
                collect_yaml_paths(child, &child_path, depth + 1, output);
            }
        }
        YamlValue::Sequence(items) => {
            for (index, child) in items.iter().enumerate() {
                let child_path = format!("{path}[{index}]");
                output.push(YamlPathEntry {
                    path: child_path.clone(),
                    kind: yaml_kind(child).into(),
                    preview: yaml_preview(child),
                    depth: depth + 1,
                });
                collect_yaml_paths(child, &child_path, depth + 1, output);
            }
        }
        _ => {}
    }
}

#[component]
fn YamlModelView(summary: Memo<YamlSummary>) -> impl IntoView {
    let selected = RwSignal::new(None::<String>);
    view! {
        <Show
            when=move || summary.get().error.is_some()
            fallback=move || view! {
                <div class="yaml-model-view yaml-view-content">
                    <div class="yaml-overview">
                        <div class="yaml-overview-title">
                            <span class="yaml-kicker">"DMN DEFINITIONS"</span>
                            <h3>{move || summary.get().name}</h3>
                            <p>{move || summary.get().description}</p>
                            <code>{move || summary.get().namespace}</code>
                        </div>
                        <div class="yaml-stat-grid">
                            <div><strong>{move || summary.get().inputs}</strong><span>"输入"</span></div>
                            <div><strong>{move || summary.get().decisions.len()}</strong><span>"决策"</span></div>
                            <div><strong>{move || summary.get().item_definitions}</strong><span>"数据类型"</span></div>
                            <div><strong>{move || summary.get().decision_services.len()}</strong><span>"决策服务"</span></div>
                        </div>
                    </div>
                    <YamlElementNavigator summary=summary selected=selected />
                    <YamlDataCatalog summary=summary />
                    <div class="yaml-section-heading"><span>"决策定义"</span><span>{move || format!("{} 个", summary.get().decisions.len())}</span></div>
                    <div class="yaml-decision-list">
                        <For
                            each=move || summary.get().decisions
                            key=|decision| decision.name.clone()
                            let:decision
                        >
                            <YamlDecisionView decision=decision selected=selected />
                        </For>
                    </div>
                    <div class="yaml-section-heading"><span>"业务知识模型"</span><span>{move || format!("{} 个", summary.get().bkms.len())}</span></div>
                    <div class="yaml-decision-list">
                        <For
                            each=move || summary.get().bkms
                            key=|bkm| bkm.name.clone()
                            let:bkm
                        >
                            <YamlKnowledgeModelView model=bkm selected=selected />
                        </For>
                        <Show when=move || summary.get().bkms.is_empty()>
                            <p class="yaml-empty-copy">"未定义业务知识模型"</p>
                        </Show>
                    </div>
                    <div class="yaml-section-heading"><span>"决策服务"</span><span>{move || format!("{} 个", summary.get().decision_services.len())}</span></div>
                    <div class="yaml-decision-list">
                        <For
                            each=move || summary.get().decision_services
                            key=|service| service.name.clone()
                            let:service
                        >
                            <YamlDecisionServiceView service=service selected=selected />
                        </For>
                        <Show when=move || summary.get().decision_services.is_empty()>
                            <p class="yaml-empty-copy">"未定义决策服务"</p>
                        </Show>
                    </div>
                </div>
            }
        >
            <div class="yaml-parse-error yaml-view-content">{move || summary.get().error.unwrap_or_else(|| "YAML 无法解析".into())}</div>
        </Show>
    }
}

#[component]
fn YamlDecisionView(decision: YamlDecision, selected: RwSignal<Option<String>>) -> impl IntoView {
    let has_table = !decision.rules.is_empty();
    let decision_name = decision.name.clone();
    let headers = RwSignal::new(decision.inputs.clone());
    let output_headers = RwSignal::new(decision.outputs.clone());
    let rows = RwSignal::new(
        decision
            .rules
            .clone()
            .into_iter()
            .enumerate()
            .collect::<Vec<_>>(),
    );
    view! {
        <article class=move || if selected.get().as_deref() == Some(decision_name.as_str()) { "yaml-decision-card is-selected" } else { "yaml-decision-card" }>
            <div class="yaml-decision-heading">
                <div><span class="yaml-node-badge">"D"</span><div><h4>{decision.name.clone()}</h4><p>{decision.description.clone()}</p></div></div>
                <div class="yaml-decision-meta"><span>{decision.element_type.clone()}</span><span>{decision.type_ref.clone()}</span><span>{if decision.hit_policy.is_empty() { "LITERAL".into() } else { decision.hit_policy.clone() }}</span></div>
            </div>
            <Show when=move || has_table fallback=move || view! {
                <div class="yaml-expression-card">
                    <div class="yaml-expression-type"><span>"表达式类型"</span><strong>{decision.expression_type.clone()}</strong></div>
                    <pre class="yaml-expression">{if decision.expression_summary.is_empty() { "该表达式未提供可直接预览的文本。".into() } else { decision.expression_summary.clone() }}</pre>
                </div>
            }>
                <div class="yaml-table-wrap">
                    <table class="yaml-decision-table">
                        <thead><tr><For each=move || headers.get() key=|header| header.clone() let:header><th>{header}</th></For><For each=move || output_headers.get() key=|header| format!("out-{header}") let:header><th class="yaml-output-header">{header}</th></For></tr></thead>
                        <tbody>
                            <For each=move || rows.get() key=|(index, _)| *index let:row>
                                <tr><For each=move || indexed(&row.1.inputs) key=|(index, _)| *index let:cell><td>{cell.1}</td></For><For each=move || indexed(&row.1.outputs) key=|(index, _)| format!("out-{index}") let:cell><td class="yaml-output-cell">{cell.1}</td></For></tr>
                            </For>
                        </tbody>
                    </table>
                </div>
            </Show>
        </article>
    }
}

#[component]
fn YamlKnowledgeModelView(
    model: YamlKnowledgeModel,
    selected: RwSignal<Option<String>>,
) -> impl IntoView {
    let model_name = model.name.clone();
    let parameters = model.parameters.clone();
    let has_parameters = !parameters.is_empty();
    let parameter_text = parameters.join(", ");
    view! {
        <article class=move || if selected.get().as_deref() == Some(model_name.as_str()) { "yaml-decision-card is-selected" } else { "yaml-decision-card" }>
            <div class="yaml-decision-heading">
                <div><span class="yaml-node-badge yaml-node-badge-knowledge">"K"</span><div><h4>{model.name.clone()}</h4><p>{model.description.clone()}</p></div></div>
                <div class="yaml-decision-meta"><span>{model.element_type.clone()}</span><span>{model.type_ref.clone()}</span><span>{model.expression_type.clone()}</span></div>
            </div>
            <Show when=move || has_parameters>
                <div class="yaml-definition-line"><strong>"参数"</strong><span>{parameter_text.clone()}</span></div>
            </Show>
            <div class="yaml-expression-card">
                <div class="yaml-expression-type"><span>"表达式预览"</span><strong>{model.expression_type.clone()}</strong></div>
                <pre class="yaml-expression">{if model.expression_summary.is_empty() { "该表达式未提供可直接预览的文本。".into() } else { model.expression_summary.clone() }}</pre>
            </div>
        </article>
    }
}

#[component]
fn YamlDecisionServiceView(
    service: YamlDecisionService,
    selected: RwSignal<Option<String>>,
) -> impl IntoView {
    let service_name = service.name.clone();
    view! {
        <article class=move || if selected.get().as_deref() == Some(service_name.as_str()) { "yaml-decision-card is-selected" } else { "yaml-decision-card" }>
            <div class="yaml-decision-heading">
                <div><span class="yaml-node-badge yaml-node-badge-service">"S"</span><div><h4>{service.name.clone()}</h4><p>{service.description.clone()}</p></div></div>
                <div class="yaml-decision-meta"><span>{service.element_type.clone()}</span></div>
            </div>
            <div class="yaml-expression-card">
                <div class="yaml-expression-type"><span>"服务范围"</span><strong>"decisionService"</strong></div>
                <pre class="yaml-expression">{if service.expression_summary.is_empty() { "未配置输入、封装决策或输出决策。".into() } else { service.expression_summary.clone() }}</pre>
            </div>
        </article>
    }
}

#[component]
fn YamlElementNavigator(
    summary: Memo<YamlSummary>,
    selected: RwSignal<Option<String>>,
) -> impl IntoView {
    view! {
        <div class="yaml-navigator">
            <div class="yaml-section-heading"><span>"元素导航"</span><span>"全部定义元素"</span></div>
            <div class="yaml-navigator-list">
                <For each=move || summary.get().graph_nodes key=|node| node.name.clone() children=move |node| {
                    let node_name = node.name.clone();
                    let node_kind = node.kind.clone();
                    let class_name = node_name.clone();
                    let click_name = node_name.clone();
                    view! {
                        <button class=move || format!("yaml-nav-item yaml-nav-{} {}", node_kind, if selected.get().as_deref() == Some(class_name.as_str()) { "is-selected" } else { "" }) on:click=move |_| selected.set(Some(click_name.clone()))>
                            <span class="yaml-nav-dot"></span><span>{node_name.clone()}</span>
                        </button>
                    }
                } />
                <For each=move || {
                    summary.get().bkms.iter().map(|bkm| YamlGraphNode { name: bkm.name.clone(), kind: "knowledge".into(), ..Default::default() }).collect::<Vec<_>>()
                } key=|node| node.name.clone() children=move |node| {
                    let node_name = node.name.clone();
                    let class_name = node_name.clone();
                    let click_name = node_name.clone();
                    view! {
                        <button class=move || format!("yaml-nav-item yaml-nav-knowledge {}", if selected.get().as_deref() == Some(class_name.as_str()) { "is-selected" } else { "" }) on:click=move |_| selected.set(Some(click_name.clone()))>
                            <span class="yaml-nav-dot"></span><span>{node_name.clone()}</span>
                        </button>
                    }
                } />
                <For each=move || {
                    summary.get().decision_services.iter().map(|service| YamlGraphNode { name: service.name.clone(), kind: "service".into(), ..Default::default() }).collect::<Vec<_>>()
                } key=|node| node.name.clone() children=move |node| {
                    let node_name = node.name.clone();
                    let class_name = node_name.clone();
                    let click_name = node_name.clone();
                    view! {
                        <button class=move || format!("yaml-nav-item yaml-nav-service {}", if selected.get().as_deref() == Some(class_name.as_str()) { "is-selected" } else { "" }) on:click=move |_| selected.set(Some(click_name.clone()))>
                            <span class="yaml-nav-dot"></span><span>{node_name.clone()}</span>
                        </button>
                    }
                } />
            </div>
        </div>
    }
}

#[component]
fn YamlDependencyGraphView(summary: Memo<YamlSummary>) -> impl IntoView {
    let selected = RwSignal::new(None::<String>);
    view! {
        <div class="yaml-view-content">
            <Show
                when=move || summary.get().error.is_some()
                fallback=move || view! {
                    <YamlDependencyGraph summary=summary selected=selected />
                }
            >
                <div class="yaml-parse-error">{move || summary.get().error.unwrap_or_else(|| "YAML 无法解析".into())}</div>
            </Show>
        </div>
    }
}

#[component]
fn YamlDependencyGraph(
    summary: Memo<YamlSummary>,
    selected: RwSignal<Option<String>>,
) -> impl IntoView {
    view! {
        <div class="yaml-graph-panel">
            <div class="yaml-section-heading"><span>"决策依赖图"</span><span>{move || format!("{} 条依赖", summary.get().graph_edges.len())}</span></div>
            <svg
                class="yaml-dependency-graph"
                viewBox=move || format!("0 0 {YAML_GRAPH_WIDTH} {}", yaml_graph_height(&summary.get().graph_nodes))
                preserveAspectRatio="xMinYMin meet"
                role="img"
                aria-label="DMN 决策依赖图"
            >
                    <defs>
                        <marker id="yaml-graph-arrow" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="6" markerHeight="6" orient="auto">
                            <path d="M 0 0 L 10 5 L 0 10 z" fill="context-stroke" />
                        </marker>
                    </defs>
                    <For each=move || summary.get().graph_edges key=|edge| format!("{}->{}", edge.source, edge.target) let:edge>
                        <path
                            class=format!("yaml-graph-edge yaml-graph-edge-{}", edge.kind)
                            d=yaml_graph_edge_path(&edge)
                            marker-end="url(#yaml-graph-arrow)"
                        />
                    </For>
                    <For each=move || summary.get().graph_nodes key=|node| node.name.clone() children=move |node| {
                        let node_name = node.name.clone();
                        let node_kind = node.kind.clone();
                        let class_name = node_name.clone();
                        let click_name = node_name.clone();
                        let label = yaml_graph_label(&node_name);
                        let knowledge_points = RwSignal::new(yaml_graph_knowledge_points(&node));
                        let is_input = node_kind == "input";
                        let is_decision = node_kind == "decision";
                        let is_knowledge = node_kind == "knowledge";
                        view! {
                            <g class=move || format!("yaml-graph-node yaml-graph-node-{} {}", node_kind, if selected.get().as_deref() == Some(class_name.as_str()) { "is-selected" } else { "" }) on:click=move |_| selected.set(Some(click_name.clone()))>
                                <title>{node_name.clone()}</title>
                                <Show when=move || is_input>
                                    <rect x=node.x.to_string() y=node.y.to_string() width=YAML_GRAPH_NODE_WIDTH.to_string() height=YAML_GRAPH_NODE_HEIGHT.to_string() rx="25" ry="25" />
                                </Show>
                                <Show when=move || is_decision>
                                    <rect x=node.x.to_string() y=node.y.to_string() width=YAML_GRAPH_NODE_WIDTH.to_string() height=YAML_GRAPH_NODE_HEIGHT.to_string() rx="2" />
                                </Show>
                                <Show when=move || is_knowledge>
                                    <polygon points=move || knowledge_points.get() />
                                </Show>
                                <text x={(node.x + YAML_GRAPH_NODE_WIDTH / 2).to_string()} y={(node.y + 30).to_string()}>{label}</text>
                            </g>
                        }
                    } />
            </svg>
        </div>
    }
}

#[component]
fn YamlDataCatalog(summary: Memo<YamlSummary>) -> impl IntoView {
    view! {
        <div class=move || if summary.get().type_details.is_empty() { "yaml-catalog-grid is-single" } else { "yaml-catalog-grid" }>
            <section class="yaml-catalog-section">
                <div class="yaml-section-heading"><span>"输入数据"</span><span>{move || format!("{} 个", summary.get().input_details.len())}</span></div>
                <div class="yaml-field-list">
                    <For each=move || summary.get().input_details key=|field| field.name.clone() let:field>
                        <div class="yaml-field-row"><span class="yaml-field-dot yaml-field-input"></span><div><strong>{field.name}</strong><small>{field.element_type}{if field.description.is_empty() { String::new() } else { format!(" · {}", field.description) }}</small></div><code>{field.type_ref}</code></div>
                    </For>
                    <Show when=move || summary.get().input_details.is_empty()><p class="yaml-empty-copy">"未定义输入数据"</p></Show>
                </div>
            </section>
            <Show when=move || !summary.get().type_details.is_empty() fallback=move || view! {
                <div class="yaml-catalog-empty"><span class="yaml-field-dot yaml-field-type"></span><span>"数据类型"</span><small>"未定义自定义数据类型"</small></div>
            }>
                <section class="yaml-catalog-section">
                    <div class="yaml-section-heading"><span>"数据类型"</span><span>{move || format!("{} 个", summary.get().type_details.len())}</span></div>
                    <div class="yaml-field-list">
                        <For each=move || summary.get().type_details key=|field| field.name.clone() let:field>
                            <div class="yaml-field-row"><span class="yaml-field-dot yaml-field-type"></span><div><strong>{field.name}</strong><small>{field.element_type}{if field.description.is_empty() { String::new() } else { format!(" · {}", field.description) }}</small></div><code>{field.type_ref}</code></div>
                        </For>
                    </div>
                </section>
            </Show>
        </div>
    }
}

#[component]
fn YamlStructureView(summary: Memo<YamlSummary>) -> impl IntoView {
    let query = RwSignal::new(String::new());
    let entries = move || {
        let query_value = query.get().to_lowercase();
        let mut all = Vec::new();
        if let Some(raw) = summary.get().raw.as_ref() {
            collect_yaml_paths(raw, "", 0, &mut all);
        }
        if query_value.trim().is_empty() {
            all
        } else {
            all.into_iter()
                .filter(|entry| {
                    entry.path.to_lowercase().contains(&query_value)
                        || entry.preview.to_lowercase().contains(&query_value)
                })
                .collect()
        }
    };

    view! {
        <div class="yaml-structure-view">
            <input
                type="search"
                class="yaml-structure-search"
                placeholder="搜索路径..."
                prop:value=query
                on:input=move |event| query.set(event_target_value(&event))
            />
            <div class="yaml-structure-list">
                <For each=entries key=|entry| entry.path.clone() let:entry>
                    <div class="yaml-structure-entry" style=format!("padding-left: {}px", entry.depth * 16)>
                        <code class="yaml-path">{entry.path.clone()}</code>
                        <span class="yaml-type">{entry.kind}</span>
                        <span class="yaml-preview">{entry.preview}</span>
                    </div>
                </For>
            </div>
        </div>
    }
}
