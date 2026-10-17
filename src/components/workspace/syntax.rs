pub fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn token_span(class: &str, value: &str) -> String {
    format!("<span class=\"{class}\">{}</span>", escape_html(value))
}

fn quoted_end(source: &str, quote: char, yaml: bool) -> usize {
    let mut characters = source.char_indices().peekable();
    characters.next();
    while let Some((offset, character)) = characters.next() {
        if character == '\\' && quote == '"' {
            characters.next();
        } else if character == quote {
            if yaml && quote == '\'' && characters.peek().is_some_and(|(_, next)| *next == quote) {
                characters.next();
            } else {
                return offset + character.len_utf8();
            }
        }
    }
    source.len()
}

fn scalar_class(value: &str) -> &'static str {
    match value {
        "true" | "false" | "True" | "False" | "TRUE" | "FALSE" => "syntax-boolean",
        "null" | "Null" | "NULL" | "~" => "syntax-null",
        _ if value.parse::<f64>().is_ok()
            || ["0x", "0o", "0b"]
                .iter()
                .any(|prefix| value.starts_with(prefix)) =>
        {
            "syntax-number"
        }
        _ => "syntax-string",
    }
}

fn highlight_scalars(source: &str, yaml: bool) -> String {
    let mut output = String::new();
    let mut offset = 0;
    while offset < source.len() {
        let remaining = &source[offset..];
        let character = remaining.chars().next().unwrap();
        if character.is_whitespace() {
            output.push(character);
            offset += character.len_utf8();
        } else if yaml
            && character == '#'
            && (offset == 0 || source[..offset].ends_with(char::is_whitespace))
        {
            output.push_str(&token_span("syntax-comment", remaining));
            break;
        } else if character == '"' || (yaml && character == '\'') {
            let length = quoted_end(remaining, character, yaml);
            let class = if remaining[length..].trim_start().starts_with(':') {
                "syntax-key"
            } else {
                "syntax-string"
            };
            output.push_str(&token_span(class, &remaining[..length]));
            offset += length;
        } else if "{}[],:".contains(character)
            || (yaml && character == '-' && remaining[1..].starts_with(char::is_whitespace))
        {
            output.push_str(&token_span(
                "syntax-punctuation",
                &remaining[..character.len_utf8()],
            ));
            offset += character.len_utf8();
        } else {
            let length = remaining
                .char_indices()
                .skip(1)
                .find_map(|(index, next)| {
                    (next.is_whitespace()
                        || "{}[],".contains(next)
                        || (next == ':'
                            && (!yaml
                                || remaining[index + 1..].starts_with(char::is_whitespace)
                                || index + 1 == remaining.len())))
                    .then_some(index)
                })
                .unwrap_or(remaining.len());
            let token = &remaining[..length];
            let class = if remaining[length..].trim_start().starts_with(':') {
                "syntax-key"
            } else if yaml
                && (token.starts_with('&')
                    || token.starts_with('*')
                    || token.starts_with('!')
                    || token.starts_with('|')
                    || token.starts_with('>')
                    || token == "---"
                    || token == "...")
            {
                "syntax-meta"
            } else {
                scalar_class(token)
            };
            output.push_str(&token_span(class, token));
            offset += length;
        }
    }
    output
}

pub fn highlight_value(value: &str, comment_marker: Option<char>) -> String {
    highlight_scalars(value, comment_marker == Some('#'))
}

pub fn highlight_yaml_line(line: &str) -> String {
    highlight_scalars(line, true)
}

fn highlight_yaml(source: &str) -> String {
    let mut block_indent = None;
    let mut output = String::new();
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if block_indent.is_some_and(|parent| indent > parent || trimmed.is_empty()) {
            output.push_str(&token_span("syntax-string", line));
        } else {
            let highlighted = highlight_yaml_line(line);
            block_indent = if highlighted.contains("class=\"syntax-meta\">|")
                || highlighted.contains("class=\"syntax-meta\">&gt;")
            {
                Some(indent)
            } else {
                None
            };
            output.push_str(&highlighted);
        }
    }
    output
}

fn xml_tag_end(source: &str) -> usize {
    let mut quote = None;
    let mut brackets = 0;
    for (offset, character) in source.char_indices() {
        match (quote, character) {
            (Some(current), next) if current == next => quote = None,
            (None, '\'' | '"') => quote = Some(character),
            (None, '[') => brackets += 1,
            (None, ']') => brackets -= 1,
            (None, '>') if brackets == 0 => return offset + 1,
            _ => {}
        }
    }
    source.len()
}

fn highlight_xml_tag(source: &str) -> String {
    let mut output = String::new();
    let mut offset = 0;
    let mut is_name = true;
    while offset < source.len() {
        let remaining = &source[offset..];
        let character = remaining.chars().next().unwrap();
        if character.is_whitespace() {
            output.push(character);
            offset += character.len_utf8();
        } else if "<>=/?".contains(character) {
            output.push_str(&token_span("syntax-punctuation", &remaining[..1]));
            offset += 1;
        } else if character == '\'' || character == '"' {
            let length = quoted_end(remaining, character, false);
            output.push_str(&token_span("syntax-attribute-value", &remaining[..length]));
            offset += length;
        } else {
            let length = remaining
                .find(|next: char| next.is_whitespace() || "<>=/?\"'".contains(next))
                .unwrap_or(remaining.len());
            output.push_str(&token_span(
                if is_name {
                    "syntax-tag-name"
                } else {
                    "syntax-attribute"
                },
                &remaining[..length],
            ));
            is_name = false;
            offset += length;
        }
    }
    output
}

fn highlight_xml_content(source: &str) -> String {
    let mut output = String::new();
    let mut offset = 0;
    while let Some(relative) = source[offset..].find('&') {
        let start = offset + relative;
        output.push_str(&escape_html(&source[offset..start]));
        let Some(end) = source[start..].find(';') else {
            output.push_str(&escape_html(&source[start..]));
            return output;
        };
        let end = start + end + 1;
        output.push_str(&token_span("syntax-entity", &source[start..end]));
        offset = end;
    }
    output.push_str(&escape_html(&source[offset..]));
    output
}

pub fn highlight_xml_text(source: &str) -> String {
    highlight_xml(source)
}

pub fn highlight_xml(source: &str) -> String {
    let mut output = String::new();
    let mut offset = 0;
    while offset < source.len() {
        let remaining = &source[offset..];
        if remaining.starts_with('<') {
            let special = if remaining.starts_with("<!--") {
                Some(("-->", "syntax-comment"))
            } else if remaining.starts_with("<![CDATA[") {
                Some(("]]>", "syntax-cdata"))
            } else {
                None
            };
            let (length, class) = match special {
                Some((ending, class)) => (
                    remaining
                        .find(ending)
                        .map(|end| end + ending.len())
                        .unwrap_or(remaining.len()),
                    Some(class),
                ),
                None => (
                    xml_tag_end(remaining),
                    remaining.starts_with("<!").then_some("syntax-meta"),
                ),
            };
            let tag = &remaining[..length];
            output.push_str(
                &class
                    .map(|class| token_span(class, tag))
                    .unwrap_or_else(|| highlight_xml_tag(tag)),
            );
            offset += length;
        } else {
            let length = remaining.find('<').unwrap_or(remaining.len());
            output.push_str(&highlight_xml_content(&remaining[..length]));
            offset += length;
        }
    }
    output
}

pub fn highlight_json(source: &str) -> String {
    highlight_scalars(source, false)
}

pub fn highlight_code(source: &str, language: &str) -> String {
    match language {
        "yaml" | "yml" => highlight_yaml(source),
        "xml" | "dmn" => highlight_xml(source),
        "json" => highlight_json(source),
        _ => escape_html(source),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn original_text(highlighted: &str) -> String {
        let mut text = String::new();
        let mut in_tag = false;
        for character in highlighted.chars() {
            match character {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => text.push(character),
                _ => {}
            }
        }
        text.replace("&quot;", "\"")
            .replace("&gt;", ">")
            .replace("&lt;", "<")
            .replace("&amp;", "&")
    }

    #[test]
    fn yaml_highlights_keys_scalars_and_quoted_hashes() {
        let source = "- name: '策略 #1'\n  enabled: true\n  count: 12.5\n  value: null # 注释\n  url: https://example.com/#item\n  expression: 'it''s # valid'";
        let html = highlight_code(source, "yaml");
        for token in [
            "syntax-key\">name",
            "syntax-string\">'策略 #1'",
            "syntax-boolean\">true",
            "syntax-number\">12.5",
            "syntax-null\">null",
            "syntax-comment\"># 注释",
        ] {
            assert!(html.contains(token), "missing {token}");
        }
        assert!(!html.contains("syntax-comment\">#1"));
        assert_eq!(original_text(&html), source);
    }

    #[test]
    fn yaml_block_scalars_are_not_treated_as_keys_or_comments() {
        let source = "expression: |\n  if age > 18: true # FEEL\n  else false\nnext: 2\n";
        let html = highlight_code(source, "yaml");
        assert!(!html.contains("syntax-comment"));
        assert!(html.contains("syntax-key\">next"));
        assert_eq!(original_text(&html), source);
    }

    #[test]
    fn xml_highlights_names_attributes_comments_and_cdata() {
        let source = "<?xml version=\"1.0\"?>\n<!-- decision\nmodel -->\n  <dmn:decision\n name=\"Loan &amp; Approval > 0\"><![CDATA[1 < 2]]></dmn:decision>";
        let html = highlight_xml(source);
        for token in [
            "syntax-tag-name\">xml",
            "syntax-attribute\">version",
            "syntax-attribute-value\">&quot;1.0&quot;",
            "syntax-comment",
            "syntax-cdata",
            "syntax-tag-name\">dmn:decision",
            "Loan &amp;amp; Approval &gt; 0",
        ] {
            assert!(html.contains(token), "missing {token}");
        }
        assert!(!html.contains("<dmn:decision"));
        assert_eq!(original_text(&html), source);
    }

    #[test]
    fn json_preserves_indentation_and_highlights_each_scalar() {
        let source =
            "{\n  \"Age\": 35, \"approved\": true, \"reason\": null, \"note\": \"A < B\"\n}";
        let html = highlight_json(source);
        for token in [
            "syntax-key\">&quot;Age&quot;",
            "syntax-number\">35",
            "syntax-boolean\">true",
            "syntax-null\">null",
            "A &lt; B",
        ] {
            assert!(html.contains(token), "missing {token}");
        }
        assert_eq!(original_text(&html), source);
    }

    #[test]
    fn source_is_preserved_for_empty_incomplete_and_unsafe_input() {
        for source in [
            "",
            "\r\n",
            "  <tag attr=\"a > b",
            "&lt; <script>alert('x')</script>",
            "文本: \"<img onerror='bad'>\"",
            "name: ['a:b', null]",
        ] {
            for language in ["yaml", "xml", "json", "text"] {
                assert_eq!(original_text(&highlight_code(source, language)), source);
            }
        }
    }

    #[test]
    fn real_artifacts_keep_whitespace_and_deep_expressions() {
        for (source, language) in [
            (
                include_str!("../../../resource/templates/loan-approval.yaml"),
                "yaml",
            ),
            (include_str!("../../../resource/loan_0812.dmn"), "xml"),
        ] {
            let html = highlight_code(source, language);
            assert!(html.contains("syntax-"));
            assert_eq!(original_text(&html), source);
        }
    }
}
