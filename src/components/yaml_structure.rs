use std::collections::{HashMap, HashSet};

use serde_yaml::Value;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct YamlPathEntry {
    pub path: String,
    pub label: String,
    pub kind: &'static str,
    pub preview: String,
    pub depth: usize,
    pub ancestors: Vec<String>,
    pub branch: bool,
}

fn untag(value: &Value) -> &Value {
    match value {
        Value::Tagged(tagged) => untag(&tagged.value),
        _ => value,
    }
}

fn kind(value: &Value) -> &'static str {
    match untag(value) {
        Value::Mapping(_) => "对象",
        Value::Sequence(_) => "数组",
        Value::String(_) => "字符串",
        Value::Number(_) => "数字",
        Value::Bool(_) => "布尔值",
        Value::Null => "空值",
        Value::Tagged(_) => unreachable!(),
    }
}

fn preview(value: &Value) -> String {
    match untag(value) {
        Value::Mapping(map) => format!("{} 个字段", map.len()),
        Value::Sequence(items) => format!("{} 项", items.len()),
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Null => "null".into(),
        Value::Tagged(_) => unreachable!(),
    }
}

fn key_text(value: &Value) -> String {
    match untag(value) {
        Value::String(text) => text.clone(),
        other => serde_yaml::to_string(other)
            .unwrap_or_else(|_| "?".into())
            .trim()
            .to_string(),
    }
}

fn child_path(parent: &str, key: &str, string_key: bool) -> String {
    if string_key
        && !key.is_empty()
        && key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
        && !key.starts_with(|character: char| character.is_ascii_digit())
    {
        if parent.is_empty() {
            key.to_string()
        } else {
            format!("{parent}.{key}")
        }
    } else if string_key {
        format!(
            "{parent}[{}]",
            serde_json::to_string(key).unwrap_or_default()
        )
    } else {
        format!("{parent}[{key}:yaml]")
    }
}

pub(super) fn yaml_paths(value: &Value) -> Vec<YamlPathEntry> {
    fn visit(value: &Value, parent: &str, ancestors: &[String], output: &mut Vec<YamlPathEntry>) {
        let mut next_ancestors = ancestors.to_vec();
        if !parent.is_empty() {
            next_ancestors.push(parent.to_string());
        }
        let children: Vec<(String, &Value, bool)> = match untag(value) {
            Value::Mapping(map) => map
                .iter()
                .map(|(key, child)| (key_text(key), child, matches!(untag(key), Value::String(_))))
                .collect(),
            Value::Sequence(items) => items
                .iter()
                .enumerate()
                .map(|(index, child)| (format!("[{index}]"), child, false))
                .collect(),
            _ => return,
        };
        for (label, child, string_key) in children {
            let path = if label.starts_with('[') && matches!(untag(value), Value::Sequence(_)) {
                format!("{parent}{label}")
            } else {
                child_path(parent, &label, string_key)
            };
            let branch = matches!(untag(child), Value::Mapping(_) | Value::Sequence(_));
            output.push(YamlPathEntry {
                path: path.clone(),
                label,
                kind: kind(child),
                preview: preview(child),
                depth: next_ancestors.len() + 1,
                ancestors: next_ancestors.clone(),
                branch,
            });
            visit(child, &path, &next_ancestors, output);
        }
    }

    let mut output = Vec::new();
    visit(value, "", &[], &mut output);
    output
}

pub(super) fn visible_paths(
    entries: &[YamlPathEntry],
    query: &str,
    roots_open: bool,
    expand_all: bool,
    toggled: &HashSet<String>,
) -> Vec<YamlPathEntry> {
    let query = query.trim().to_lowercase();
    let matching: HashSet<&str> = entries
        .iter()
        .filter(|entry| {
            !query.is_empty()
                && (entry.path.to_lowercase().contains(&query)
                    || entry.preview.to_lowercase().contains(&query))
        })
        .flat_map(|entry| {
            std::iter::once(entry.path.as_str()).chain(entry.ancestors.iter().map(String::as_str))
        })
        .collect();
    let branches: HashSet<&str> = entries
        .iter()
        .filter(|entry| entry.branch)
        .map(|entry| entry.path.as_str())
        .collect();
    let depths: HashMap<&str, usize> = entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry.depth))
        .collect();
    entries
        .iter()
        .filter(|entry| {
            if !query.is_empty() {
                return matching.contains(entry.path.as_str());
            }
            entry.ancestors.iter().all(|ancestor| {
                let depth = depths.get(ancestor.as_str()).copied().unwrap_or(0);
                !branches.contains(ancestor.as_str())
                    || (if expand_all {
                        true
                    } else {
                        depth == 1 && roots_open
                    }) != toggled.contains(ancestor)
            })
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_nodes_scalars_and_distinct_paths_are_preserved() {
        let yaml: Value = serde_yaml::from_str(
            "a.b: 1\na:\n  b: false\n  items:\n    - rule:\n        expression: null\nquoted[key]: 'ok'\n'1': text\n1: numeric\n",
        )
        .unwrap();
        let entries = yaml_paths(&yaml);
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.path == "[\"a.b\"]")
                .unwrap()
                .preview,
            "1"
        );
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.path == "a.b")
                .unwrap()
                .preview,
            "false"
        );
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.path == "a.items[0].rule.expression")
                .unwrap()
                .preview,
            "null"
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry.path == "[\"quoted[key]\"]")
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry.path == "[\"1\"]" && entry.preview == "text")
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry.path == "[1:yaml]" && entry.preview == "numeric")
        );
    }

    #[test]
    fn search_shows_ancestors_and_expand_modes_control_children() {
        let yaml: Value =
            serde_yaml::from_str("decisions:\n  - rules:\n      - output: true\n").unwrap();
        let entries = yaml_paths(&yaml);
        let none = HashSet::new();
        assert_eq!(visible_paths(&entries, "", true, false, &none).len(), 2);
        assert_eq!(
            visible_paths(&entries, "output", false, false, &none).len(),
            entries.len()
        );
        assert_eq!(
            visible_paths(&entries, "", true, true, &none).len(),
            entries.len()
        );
        assert_eq!(visible_paths(&entries, "", false, false, &none).len(), 1);
        let toggled = HashSet::from(["decisions".to_string()]);
        assert_eq!(visible_paths(&entries, "", true, false, &toggled).len(), 1);
    }
}
