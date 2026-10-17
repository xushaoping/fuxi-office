pub(super) fn field_label(label: &str, name: &str) -> String {
    if !label.trim().is_empty() {
        label.trim().to_string()
    } else if !name.trim().is_empty() {
        name.trim().to_string()
    } else {
        "未命名信息".into()
    }
}

pub(super) fn business_type(type_ref: &str) -> &'static str {
    match type_ref.trim().to_ascii_lowercase().as_str() {
        "number" | "integer" | "double" | "decimal" | "long" => "数值",
        "string" => "文字",
        "boolean" | "bool" => "是 / 否",
        "date" => "日期",
        "time" => "时间",
        "datetime" | "date and time" => "日期与时间",
        "list" | "array" => "多项信息",
        "context" | "object" => "组合信息",
        _ => "业务信息",
    }
}

pub(super) fn business_risk(level: &str) -> (&'static str, &'static str) {
    match level.trim().to_ascii_lowercase().as_str() {
        "high" | "高" | "高风险" => ("高风险，建议重点复核", "high"),
        "medium" | "中" | "中风险" => ("中风险，注意业务边界", "medium"),
        "low" | "低" | "低风险" => ("低风险，仍需核对规则", "low"),
        _ => ("风险级别待评估", "unknown"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_prefer_business_names_without_losing_fallbacks() {
        assert_eq!(field_label(" 客户年龄 ", "Age"), "客户年龄");
        assert_eq!(field_label("  ", "Age"), "Age");
        assert_eq!(field_label("", ""), "未命名信息");
    }

    #[test]
    fn technical_types_are_presented_as_business_information() {
        assert_eq!(business_type(" number "), "数值");
        assert_eq!(business_type("boolean"), "是 / 否");
        assert_eq!(business_type("dateTime"), "日期与时间");
        assert_eq!(business_type("CustomerProfile"), "业务信息");
    }

    #[test]
    fn risks_are_localized_and_unknown_levels_are_not_assumed_safe() {
        assert_eq!(business_risk("HIGH").1, "high");
        assert_eq!(business_risk("medium").1, "medium");
        assert_eq!(business_risk("low").1, "low");
        assert_eq!(business_risk("").1, "unknown");
        assert_eq!(business_risk("unrated").1, "unknown");
    }
}
