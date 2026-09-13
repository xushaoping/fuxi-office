use serde_yaml::Value as YamlValue;

pub const YAML_GRAPH_WIDTH: i32 = 800;
pub const YAML_GRAPH_NODE_WIDTH: i32 = 156;
pub const YAML_GRAPH_NODE_HEIGHT: i32 = 50;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YamlGraphNode {
    pub name: String,
    pub kind: String,
    pub x: i32,
    pub y: i32,
}

impl Default for YamlGraphNode {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: String::new(),
            x: 0,
            y: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YamlGraphEdge {
    pub source: String,
    pub target: String,
    pub kind: String,
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
}

pub fn yaml_graph_height(nodes: &[YamlGraphNode]) -> i32 {
    nodes
        .iter()
        .map(|node| node.y + YAML_GRAPH_NODE_HEIGHT + 34)
        .max()
        .unwrap_or(260)
        .max(260)
}

pub fn yaml_graph_edge_anchors(
    source: &YamlGraphNode,
    target: &YamlGraphNode,
) -> (i32, i32, i32, i32) {
    if source.x < target.x {
        (
            source.x + YAML_GRAPH_NODE_WIDTH,
            source.y + YAML_GRAPH_NODE_HEIGHT / 2,
            target.x,
            target.y + YAML_GRAPH_NODE_HEIGHT / 2,
        )
    } else if source.x > target.x {
        (
            source.x,
            source.y + YAML_GRAPH_NODE_HEIGHT / 2,
            target.x + YAML_GRAPH_NODE_WIDTH,
            target.y + YAML_GRAPH_NODE_HEIGHT / 2,
        )
    } else if source.y <= target.y {
        (
            source.x + YAML_GRAPH_NODE_WIDTH / 2,
            source.y + YAML_GRAPH_NODE_HEIGHT,
            target.x + YAML_GRAPH_NODE_WIDTH / 2,
            target.y,
        )
    } else {
        (
            source.x + YAML_GRAPH_NODE_WIDTH / 2,
            source.y,
            target.x + YAML_GRAPH_NODE_WIDTH / 2,
            target.y + YAML_GRAPH_NODE_HEIGHT,
        )
    }
}

pub fn yaml_graph_edge_path(edge: &YamlGraphEdge) -> String {
    if edge.x1 == edge.x2 {
        let control = (edge.y2 - edge.y1) as f32 * 0.45;
        format!(
            "M {} {} C {} {:.1}, {} {:.1}, {} {}",
            edge.x1,
            edge.y1,
            edge.x1,
            edge.y1 as f32 + control,
            edge.x2,
            edge.y2 as f32 - control,
            edge.x2,
            edge.y2
        )
    } else {
        let control = (edge.x2 - edge.x1) as f32 * 0.45;
        format!(
            "M {} {} C {:.1} {}, {:.1} {}, {} {}",
            edge.x1,
            edge.y1,
            edge.x1 as f32 + control,
            edge.y1,
            edge.x2 as f32 - control,
            edge.y2,
            edge.x2,
            edge.y2
        )
    }
}

pub fn yaml_graph_label(name: &str) -> String {
    const MAX_CHARS: usize = 18;
    let mut chars = name.chars();
    let label = chars.by_ref().take(MAX_CHARS).collect::<String>();
    if chars.next().is_some() {
        format!("{label}...")
    } else {
        label
    }
}

pub fn yaml_graph_knowledge_points(node: &YamlGraphNode) -> String {
    let x = node.x;
    let y = node.y;
    let right = x + YAML_GRAPH_NODE_WIDTH;
    let bottom = y + YAML_GRAPH_NODE_HEIGHT;
    format!(
        "{},{} {},{} {},{} {},{} {},{} {},{}",
        x + 16,
        y,
        x,
        y + 16,
        x,
        bottom,
        right - 16,
        bottom,
        right,
        bottom - 16,
        right,
        y
    )
}

pub fn yaml_map(value: &YamlValue) -> Option<&serde_yaml::Mapping> {
    match value {
        YamlValue::Mapping(map) => Some(map),
        _ => None,
    }
}

pub fn yaml_get<'a>(map: &'a serde_yaml::Mapping, key: &str) -> Option<&'a YamlValue> {
    map.get(&YamlValue::String(key.into()))
}

pub fn yaml_text(value: Option<&YamlValue>) -> String {
    value
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

pub fn yaml_list(value: Option<&YamlValue>) -> Vec<YamlValue> {
    value
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.clone())
        .unwrap_or_default()
}

pub fn yaml_joined_list(value: Option<&YamlValue>) -> String {
    yaml_list(value)
        .iter()
        .filter_map(|item| item.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn yaml_kind(value: &YamlValue) -> &'static str {
    match value {
        YamlValue::String(_) => "string",
        YamlValue::Number(_) => "number",
        YamlValue::Bool(_) => "boolean",
        YamlValue::Sequence(_) => "list",
        YamlValue::Mapping(_) => "context",
        YamlValue::Null => "null",
        _ => "unknown",
    }
}

pub fn yaml_preview(value: &YamlValue) -> String {
    match value {
        YamlValue::String(s) => {
            if s.len() > 40 {
                format!("{}...", &s[..40])
            } else {
                s.clone()
            }
        }
        YamlValue::Number(n) => n.to_string(),
        YamlValue::Bool(b) => b.to_string(),
        YamlValue::Sequence(seq) => format!("[{} items]", seq.len()),
        YamlValue::Mapping(map) => format!("{{ {} keys }}", map.len()),
        YamlValue::Null => "null".to_string(),
        _ => "...".to_string(),
    }
}
