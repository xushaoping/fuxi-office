const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

pub(super) fn embed(text: &str, dimensions: usize) -> Vec<f32> {
    let mut vector = vec![0.0; dimensions];
    if dimensions == 0 {
        return vector;
    }

    for feature in features(text) {
        let hash = stable_hash(&feature);
        let slot = hash as usize % dimensions;
        let sign = if hash & (1 << 63) == 0 { 1.0 } else { -1.0 };
        vector[slot] += sign;
    }

    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        vector.iter_mut().for_each(|value| *value /= norm);
    }
    vector
}

fn features(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for character in text.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            current.push(character);
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    let mut result = Vec::new();
    for token in &tokens {
        let characters: Vec<char> = token.chars().collect();
        if characters.iter().all(|character| is_cjk(*character)) {
            result.extend(characters.iter().map(|character| format!("c:{character}")));
            result.extend(
                characters
                    .windows(2)
                    .map(|pair| format!("b:{}{}", pair[0], pair[1])),
            );
        } else {
            result.push(format!("w:{token}"));
            result.extend(
                characters
                    .windows(3)
                    .map(|gram| format!("g:{}{}{}", gram[0], gram[1], gram[2])),
            );
        }
    }
    result.extend(
        tokens
            .windows(2)
            .map(|pair| format!("p:{}_{}", pair[0], pair[1])),
    );
    result
}

fn is_cjk(character: char) -> bool {
    matches!(
        character as u32,
        0x3400..=0x4dbf
            | 0x4e00..=0x9fff
            | 0xf900..=0xfaff
            | 0x20000..=0x2fa1f
            | 0x3040..=0x30ff
            | 0xac00..=0xd7af
    )
}

fn stable_hash(value: &str) -> u64 {
    value
        .as_bytes()
        .iter()
        .fold(FNV_OFFSET_BASIS, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
        })
}

#[cfg(test)]
mod tests {
    use super::embed;

    #[test]
    fn vectors_are_deterministic_normalized_and_dimensioned() {
        let first = embed("订单金额审批", 1024);
        let second = embed("订单金额审批", 1024);
        assert_eq!(first, second);
        assert_eq!(first.len(), 1024);
        let norm = first.iter().map(|value| value * value).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6);
    }

    #[test]
    fn shared_terms_produce_a_relevant_cosine_similarity() {
        let left = embed("订单金额超过一万元需要审批", 1024);
        let right = embed("订单金额审批流程", 1024);
        let similarity = left.iter().zip(right).map(|(a, b)| a * b).sum::<f32>();
        assert!(similarity >= 0.5, "similarity was {similarity}");
    }

    #[test]
    fn empty_text_produces_a_zero_vector() {
        assert!(embed(" !!! ", 1024).iter().all(|value| *value == 0.0));
    }
}
