/// Agent 活动描述转换
pub fn agent_activity_for_step(step: &str) -> String {
    if step.contains("等待首段输出") {
        "正在等待模型返回首段内容".to_string()
    } else if step.contains("ChangeIntent") || step.contains("变更意图") {
        "正在生成并校验受控模型变更意图".to_string()
    } else if step.contains("调用 LLM") {
        "正在调用语言模型生成 DSL".to_string()
    } else if step.contains("校验") {
        "正在校验 DSL 与 DMN 结构".to_string()
    } else if step.contains("编译") {
        "正在编译 DMN XML".to_string()
    } else if step.contains("重试") {
        "正在修复生成结果并重新尝试".to_string()
    } else {
        step.trim_start_matches(['→', '✓', '⚠', '…', ' '])
            .to_string()
    }
}

/// Timeline 项目样式类
pub fn timeline_item_class(step: &str, is_current: bool) -> &'static str {
    if step.starts_with('✓') {
        "is-complete"
    } else if step.starts_with('⚠') {
        "is-warning"
    } else if step.starts_with('…') {
        "is-waiting"
    } else if is_current {
        "is-active"
    } else {
        "is-past"
    }
}

/// Timeline 项目文本
pub fn timeline_item_text(step: &str) -> String {
    step.trim_start_matches(['→', '✓', '⚠', '…', ' '])
        .to_string()
}
