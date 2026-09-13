use crate::api::ChangeSummary;

pub fn apply_result_message(summary: Option<&ChangeSummary>, dsl_length: usize) -> String {
    summary
        .map(|s| incremental_result_message(s, dsl_length))
        .unwrap_or_else(|| format!("模型已生成并通过校验。DSL 共 {} 个字符。", dsl_length))
}

pub fn incremental_result_message(summary: &ChangeSummary, dsl_length: usize) -> String {
    let mut changes = Vec::new();
    if !summary.added.is_empty() {
        changes.push(format!("新增 {}", summary_names(&summary.added)));
    }
    if !summary.updated.is_empty() {
        changes.push(format!("更新 {}", summary_names(&summary.updated)));
    }
    if !summary.removed.is_empty() {
        changes.push(format!("移除 {}", summary_names(&summary.removed)));
    }
    if changes.is_empty() {
        format!("模型已更新，DSL 共 {} 个字符。", dsl_length)
    } else {
        format!(
            "模型已更新（{}），DSL 共 {} 个字符。",
            changes.join("，"),
            dsl_length
        )
    }
}

pub fn summary_names(names: &[String]) -> String {
    match names.len() {
        0 => String::new(),
        1 => names[0].clone(),
        2 => format!("{} 和 {}", names[0], names[1]),
        _ => format!("{}、{} 等 {} 项", names[0], names[1], names.len()),
    }
}

pub fn indexed(values: &[String]) -> Vec<(usize, String)> {
    values
        .iter()
        .enumerate()
        .map(|(i, v)| (i, v.clone()))
        .collect()
}

pub fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn token_span(class: &str, value: &str) -> String {
    format!("<span class=\"{}\">{}</span>", class, escape_html(value))
}

pub fn highlight_value(value: &str, comment_marker: Option<char>) -> String {
    if let Some(marker) = comment_marker {
        if let Some(pos) = value.find(marker) {
            let (code, comment) = value.split_at(pos);
            return format!(
                "{}{}",
                escape_html(code),
                token_span("token-comment", comment)
            );
        }
    }
    escape_html(value)
}

pub fn highlight_yaml_line(line: &str) -> String {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];

    if trimmed.starts_with('#') {
        return format!("{}{}", indent, token_span("token-comment", trimmed));
    }

    if let Some(colon_pos) = trimmed.find(':') {
        let key = &trimmed[..colon_pos];
        let rest = &trimmed[colon_pos..];
        return format!(
            "{}{}{}",
            indent,
            token_span("token-key", key),
            highlight_value(rest, Some('#'))
        );
    }

    highlight_value(line, Some('#'))
}

pub fn highlight_xml_text(value: &str) -> String {
    let mut result = String::new();
    let mut start = 0;

    while start < value.len() {
        if let Some(tag_start) = value[start..].find('<') {
            let before = &value[start..start + tag_start];
            if !before.trim().is_empty() {
                result.push_str(&escape_html(before));
            }

            start += tag_start;
            let tag_end = xml_token_end(&value[start..]);
            let tag = &value[start..start + tag_end];
            result.push_str(&highlight_xml_tag(tag));
            start += tag_end;
        } else {
            result.push_str(&escape_html(&value[start..]));
            break;
        }
    }

    result
}

fn xml_token_end(value: &str) -> usize {
    value.find('>').map(|pos| pos + 1).unwrap_or(value.len())
}

fn highlight_xml_tag(tag: &str) -> String {
    if tag.starts_with("<!--") {
        return token_span("token-comment", tag);
    }

    if tag.starts_with("<?") || tag.starts_with("<!") {
        return token_span("token-meta", tag);
    }

    // Simple tag highlighting
    token_span("token-tag", tag)
}

pub fn highlight_xml(source: &str) -> String {
    source
        .lines()
        .map(|line| format!("{}\n", highlight_xml_text(line)))
        .collect()
}

pub fn highlight_json(source: &str) -> String {
    source
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('"') && trimmed.contains(':') {
                // Key-value pair
                if let Some(colon) = trimmed.find(':') {
                    let key_part = &trimmed[..colon + 1];
                    let value_part = &trimmed[colon + 1..];
                    format!(
                        "{}{}\n",
                        token_span("token-key", key_part),
                        escape_html(value_part)
                    )
                } else {
                    format!("{}\n", escape_html(line))
                }
            } else {
                format!("{}\n", escape_html(line))
            }
        })
        .collect()
}

pub fn highlight_code(source: &str, language: &str) -> String {
    match language {
        "yaml" => source
            .lines()
            .map(|line| format!("{}\n", highlight_yaml_line(line)))
            .collect(),
        "xml" => highlight_xml(source),
        "json" => highlight_json(source),
        _ => escape_html(source),
    }
}
