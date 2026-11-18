use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub content: String,
    pub token_count: usize,
    pub metadata: Value,
}

#[derive(Debug, Clone, Copy)]
pub struct KnowledgeChunker {
    target_tokens: usize,
    overlap_tokens: usize,
}

impl KnowledgeChunker {
    pub fn new(target_tokens: usize, overlap_tokens: usize) -> Self {
        Self {
            target_tokens: target_tokens.max(1),
            overlap_tokens: overlap_tokens.min(target_tokens.max(1) - 1),
        }
    }

    pub fn chunk(&self, content: &str) -> Vec<Chunk> {
        let mut sections: Vec<(String, String)> = Vec::new();
        let mut heading = String::new();
        let mut text = String::new();

        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(section_heading) = section_heading(trimmed) {
                if !text.trim().is_empty() {
                    sections.push((heading, std::mem::take(&mut text)));
                }
                heading = section_heading.to_string();
            }
            if !trimmed.is_empty() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(trimmed);
            }
        }
        if !text.trim().is_empty() {
            sections.push((heading, text));
        }

        let mut chunks = Vec::new();
        for (section, text) in sections {
            if estimate_tokens(&text) <= self.target_tokens {
                chunks.push(make_chunk(text, &section));
            } else {
                chunks.extend(self.chunk_section(&text, &section));
            }
        }
        chunks
    }

    fn chunk_section(&self, text: &str, section: &str) -> Vec<Chunk> {
        let mut atoms = Vec::new();
        for paragraph in text.split('\n') {
            if estimate_tokens(paragraph) <= self.target_tokens {
                atoms.push(paragraph.to_string());
            } else {
                atoms.extend(split_long_text(
                    paragraph,
                    self.target_tokens
                        .saturating_sub(self.overlap_tokens)
                        .max(1),
                ));
            }
        }

        let mut chunks = Vec::new();
        let mut current = String::new();
        for atom in atoms {
            let candidate = if current.is_empty() {
                atom.clone()
            } else {
                format!("{current}\n{atom}")
            };
            if estimate_tokens(&candidate) <= self.target_tokens {
                current = candidate;
                continue;
            }
            if !current.is_empty() {
                chunks.push(make_chunk(current.clone(), section));
            }
            let mut overlap = overlap_tail(&current, self.overlap_tokens);
            while !overlap.is_empty()
                && estimate_tokens(&format!("{overlap}\n{atom}")) > self.target_tokens
            {
                overlap.remove(0);
            }
            current = if overlap.is_empty() {
                atom
            } else {
                format!("{overlap}\n{atom}")
            };
        }
        if !current.is_empty() {
            chunks.push(make_chunk(current, section));
        }
        chunks
    }
}

fn section_heading(line: &str) -> Option<&str> {
    if line.starts_with('#') {
        return Some(line);
    }
    if let Some(article_heading) = article_heading(line) {
        return Some(article_heading);
    }
    line.split_once(['.', '、', ')', '）'])
        .filter(|(prefix, _)| !prefix.is_empty() && prefix.chars().all(char::is_numeric))
        .map(|_| line)
}

fn article_heading(line: &str) -> Option<&str> {
    let Some(article_number) = line.strip_prefix('第') else {
        return None;
    };
    let Some((article_number, suffix)) = article_number.split_once('条') else {
        return None;
    };
    if suffix.starts_with('款')
        || article_number.is_empty()
        || !article_number.chars().all(|ch| {
            ch.is_ascii_digit()
                || matches!(
                    ch,
                    '〇' | '零'
                        | '一'
                        | '二'
                        | '三'
                        | '四'
                        | '五'
                        | '六'
                        | '七'
                        | '八'
                        | '九'
                        | '十'
                        | '百'
                        | '千'
                        | '万'
                        | '亿'
                        | '两'
                )
        })
    {
        return None;
    }
    let title_end = '第'.len_utf8() + article_number.len() + '条'.len_utf8();
    Some(&line[..title_end])
}

fn estimate_tokens(text: &str) -> usize {
    let mut count = 0usize;
    let mut ascii_run = 0usize;
    for ch in text.chars() {
        if is_cjk(ch) {
            count = count.saturating_add(1 + ascii_run.div_ceil(4));
            ascii_run = 0;
        } else if ch.is_ascii_alphanumeric() {
            ascii_run += 1;
        } else {
            count = count.saturating_add(ascii_run.div_ceil(4));
            ascii_run = 0;
            if !ch.is_whitespace() {
                count = count.saturating_add(1);
            }
        }
    }
    count.saturating_add(ascii_run.div_ceil(4))
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2FA1F
            | 0x3040..=0x30FF
            | 0xAC00..=0xD7AF
    )
}

fn split_long_text(text: &str, target_tokens: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        let mut candidate = current.clone();
        candidate.push(ch);
        if !current.is_empty() && estimate_tokens(&candidate) > target_tokens {
            parts.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

fn overlap_tail(text: &str, overlap_tokens: usize) -> String {
    if overlap_tokens == 0 {
        return String::new();
    }
    let mut tail = String::new();
    for ch in text.chars().rev() {
        tail.insert(0, ch);
        if estimate_tokens(&tail) >= overlap_tokens {
            break;
        }
    }
    tail
}

fn make_chunk(content: String, section: &str) -> Chunk {
    Chunk {
        token_count: estimate_tokens(&content),
        content,
        metadata: json!({"section": section}),
    }
}

#[cfg(test)]
mod tests {
    use super::KnowledgeChunker;

    #[test]
    fn splits_at_markdown_and_article_headings() {
        let chunks = KnowledgeChunker::new(200, 20).chunk(
            "# Eligibility\nBorrower must be resident.\n\n第十二条 借款人应具有完全民事行为能力。",
        );
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].metadata["section"], "# Eligibility");
        assert_eq!(chunks[1].metadata["section"], "第十二条");
    }

    #[test]
    fn article_heading_requires_a_valid_number_before_the_article_marker() {
        let chunks = KnowledgeChunker::new(200, 20)
            .chunk("第十二条 借款人应提供材料。\n第二条款说明不是新条文。");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].metadata["section"], "第十二条");
        assert!(chunks[0].content.contains("第二条款说明不是新条文。"));
    }

    #[test]
    fn bounds_long_sections_and_carries_context_overlap() {
        let text = "甲".repeat(60);
        let chunks = KnowledgeChunker::new(20, 4).chunk(&text);
        assert!(chunks.len() > 2);
        assert!(chunks.iter().all(|chunk| chunk.token_count <= 20));
        let overlap = chunks[0]
            .content
            .chars()
            .rev()
            .take(4)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        assert!(chunks[1].content.starts_with(&overlap));
    }

    #[test]
    fn preserves_content_and_empty_input_produces_no_chunks() {
        let chunks = KnowledgeChunker::new(200, 20).chunk("## Terms\nRisk means credit exposure.");
        assert!(chunks[0].content.contains("Risk means credit exposure."));
        assert!(KnowledgeChunker::new(200, 20).chunk("  \n").is_empty());
    }
}
