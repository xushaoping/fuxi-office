use sha2::{Digest, Sha256};
use sqlx::PgPool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    E1Structure,
    E2Reference,
    E3Type,
    E4Syntax,
    E5Semantic,
    E6Compilation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepairStrategy {
    pub instructions: &'static str,
    pub preserve_existing_elements: bool,
}

impl ErrorCategory {
    pub fn from_code(code: &str) -> Option<Self> {
        match code.trim() {
            "E1" => Some(Self::E1Structure),
            "E2" => Some(Self::E2Reference),
            "E3" => Some(Self::E3Type),
            "E4" => Some(Self::E4Syntax),
            "E5" => Some(Self::E5Semantic),
            "E6" => Some(Self::E6Compilation),
            _ => None,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::E1Structure => "E1",
            Self::E2Reference => "E2",
            Self::E3Type => "E3",
            Self::E4Syntax => "E4",
            Self::E5Semantic => "E5",
            Self::E6Compilation => "E6",
        }
    }

    pub fn classify(error_msg: &str) -> Option<Self> {
        let message = error_msg.to_ascii_lowercase();
        let contains_any = |needles: &[&str]| needles.iter().any(|needle| message.contains(needle));

        if contains_any(&[
            "missing field",
            "invalid name",
            "duplicate name",
            "缺少字段",
            "字段缺失",
            "名称非法",
            "名称重复",
        ]) {
            return Some(Self::E1Structure);
        }
        if contains_any(&[
            "undefined input",
            "undefined decision",
            "unresolved reference",
            "unknown reference",
            "未定义输入",
            "未定义决策",
            "引用不存在",
        ]) {
            return Some(Self::E2Reference);
        }
        if contains_any(&[
            "type mismatch",
            "incompatible type",
            "type error",
            "类型不匹配",
            "类型错误",
        ]) {
            return Some(Self::E3Type);
        }
        if contains_any(&[
            "syntax error",
            "parse error",
            "unexpected token",
            "feel parsing",
            "语法错误",
            "解析失败",
            "解析错误",
        ]) {
            return Some(Self::E4Syntax);
        }
        if contains_any(&[
            "unreachable rule",
            "rule conflict",
            "semantic validation",
            "semantic error",
            "dsl logic validation failed",
            "dsl 逻辑校验失败",
            "不可达规则",
            "规则冲突",
            "语义校验",
            "语义错误",
        ]) {
            return Some(Self::E5Semantic);
        }
        if contains_any(&[
            "dmn xml generation",
            "dmn xml generation failed",
            "xml compilation",
            "dmn compilation",
            "dmn 模型核验失败",
            "dmn xml 生成失败",
            "dmn 编译失败",
        ]) {
            return Some(Self::E6Compilation);
        }
        None
    }

    pub fn repair_strategy(self) -> RepairStrategy {
        let instructions = match self {
            Self::E1Structure => "只修复缺失、非法或重复的结构字段；保留已有元素及其业务逻辑。",
            Self::E2Reference => {
                "只修复未定义或无效的引用；保留已有元素及其业务逻辑，不臆造新依赖。"
            }
            Self::E3Type => "只修复类型声明或类型不兼容的表达式；保持规则含义和已有元素不变。",
            Self::E4Syntax => "只修复解析器指出的语法问题；保持表达式意图和已有元素不变。",
            Self::E5Semantic => "只修复导致语义校验失败的规则或依赖；不要改动无关规则。",
            Self::E6Compilation => {
                "只修复导致 DMN 生成或编译失败的字段和表达式；保持业务逻辑不变。"
            }
        };
        RepairStrategy {
            instructions,
            preserve_existing_elements: !matches!(self, Self::E5Semantic),
        }
    }
}

fn error_signature(error_msg: &str) -> String {
    let diagnostic = error_msg
        .split_once("\n\n出错位置附近的 JSON 内容片段:")
        .or_else(|| error_msg.split_once("\n\n你返回的 JSON 开头片段:"))
        .map(|(diagnostic, _)| diagnostic)
        .unwrap_or(error_msg);
    let mut tokens = Vec::new();
    let mut skip_location = false;
    for token in diagnostic.split_whitespace() {
        if skip_location {
            skip_location = false;
            continue;
        }
        if matches!(token, "line" | "column") {
            skip_location = true;
            continue;
        }
        tokens.push(token);
    }
    let normalized = tokens.join(" ").to_ascii_lowercase();
    format!("{:x}", Sha256::digest(normalized.as_bytes()))
}

pub async fn successful_memories(
    db: &PgPool,
    error_msg: &str,
    category: ErrorCategory,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT repair_summary FROM repair_memories WHERE error_signature = $1 AND error_category = $2 AND outcome = 'success' ORDER BY created_at DESC, id DESC LIMIT 3",
    )
    .bind(error_signature(error_msg))
    .bind(category.code())
    .fetch_all(db)
    .await
}

pub async fn record_outcome(
    db: &PgPool,
    error_msg: &str,
    category: ErrorCategory,
    model: &str,
    outcome: &str,
) -> Result<(), sqlx::Error> {
    let strategy = category.repair_strategy();
    let repair_diff = serde_json::json!({
        "category": category.code(),
        "instructions": strategy.instructions,
    });
    sqlx::query(
        "INSERT INTO repair_memories (error_signature, error_category, repair_summary, repair_diff, outcome, model) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(error_signature(error_msg))
    .bind(category.code())
    .bind(strategy.instructions)
    .bind(repair_diff)
    .bind(outcome)
    .bind(model)
    .execute(db)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ErrorCategory;

    #[test]
    fn golden_repair_diagnostics_match_expected_categories() {
        let corpus = include_str!("../../tests/golden/repair-diagnostics.tsv");
        let mut case_count = 0;
        for line in corpus
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            let fields = line.splitn(3, '\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 3, "malformed repair golden row: {line}");
            let [id, expected, diagnostic] =
                <[&str; 3]>::try_from(fields).expect("three fields are required");
            let actual = ErrorCategory::classify(diagnostic).map(ErrorCategory::code);
            assert_eq!(actual, (expected != "NONE").then_some(expected), "{id}");
            if let Some(category) = ErrorCategory::classify(diagnostic) {
                let strategy = category.repair_strategy();
                assert!(!strategy.instructions.trim().is_empty(), "{id}");
            }
            case_count += 1;
        }
        assert!(
            case_count >= 18,
            "repair golden suite only has {case_count} cases"
        );
    }

    #[test]
    fn classifies_known_diagnostics_in_priority_order() {
        let cases = [
            ("missing field 'name'", ErrorCategory::E1Structure),
            ("undefined decision 'RiskGrade'", ErrorCategory::E2Reference),
            ("FEEL type mismatch", ErrorCategory::E3Type),
            ("FEEL syntax error near ')'", ErrorCategory::E4Syntax),
            (
                "semantic validation: unreachable rule",
                ErrorCategory::E5Semantic,
            ),
            ("DMN XML generation failed", ErrorCategory::E6Compilation),
            ("字段缺失: name", ErrorCategory::E1Structure),
            ("规则冲突", ErrorCategory::E5Semantic),
        ];

        for (message, expected) in cases {
            assert_eq!(
                ErrorCategory::classify(message),
                Some(expected),
                "{message}"
            );
        }
    }

    #[test]
    fn leaves_unrecognized_diagnostics_unclassified() {
        assert_eq!(ErrorCategory::classify("provider request timed out"), None);
    }

    #[test]
    fn parses_only_known_repair_category_codes() {
        for (code, expected) in [
            ("E1", ErrorCategory::E1Structure),
            ("E2", ErrorCategory::E2Reference),
            ("E3", ErrorCategory::E3Type),
            ("E4", ErrorCategory::E4Syntax),
            ("E5", ErrorCategory::E5Semantic),
            ("E6", ErrorCategory::E6Compilation),
        ] {
            assert_eq!(ErrorCategory::from_code(code), Some(expected));
        }
        assert_eq!(ErrorCategory::from_code("unknown"), None);
        assert_eq!(ErrorCategory::from_code("E7"), None);
    }

    #[test]
    fn error_signature_is_stable_and_ignores_whitespace_and_case() {
        assert_eq!(
            super::error_signature("FEEL type mismatch"),
            super::error_signature(" feel   TYPE mismatch ")
        );
        assert_ne!(
            super::error_signature("FEEL type mismatch"),
            super::error_signature("FEEL syntax error")
        );
    }

    #[test]
    fn error_signature_ignores_json_excerpt_and_parser_location() {
        assert_eq!(
            super::error_signature(
                "JSON deserialization failed: missing field `name` at line 2 column 8\n\n出错位置附近的 JSON 内容片段:\n...{\"name\": null}..."
            ),
            super::error_signature(
                "json deserialization failed: missing field `name` at line 90 column 41\n\n出错位置附近的 JSON 内容片段:\n...{\"name\": \"other\"}..."
            )
        );
    }

    #[test]
    fn every_category_has_a_stable_code_and_repair_instruction() {
        let categories = [
            ErrorCategory::E1Structure,
            ErrorCategory::E2Reference,
            ErrorCategory::E3Type,
            ErrorCategory::E4Syntax,
            ErrorCategory::E5Semantic,
            ErrorCategory::E6Compilation,
        ];

        for (index, category) in categories.into_iter().enumerate() {
            let strategy = category.repair_strategy();
            assert_eq!(category.code(), ["E1", "E2", "E3", "E4", "E5", "E6"][index]);
            assert!(!strategy.instructions.is_empty());
        }
    }
}
