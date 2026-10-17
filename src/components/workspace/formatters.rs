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
