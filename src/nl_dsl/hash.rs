use anyhow::{Context, Result};

use super::Definitions;

/// Deterministic, dependency-free hash for a canonical Definitions snapshot.
/// This is an optimistic-concurrency token, not a security primitive.
pub fn definitions_hash(definitions: &Definitions) -> Result<String> {
    let bytes = serde_json::to_vec(definitions).context("序列化 DSL 基线失败")?;
    let mut hash_a: u64 = 0xcbf29ce484222325;
    let mut hash_b: u64 = 0x9e3779b185ebca87;
    for byte in bytes {
        hash_a ^= u64::from(byte);
        hash_a = hash_a.wrapping_mul(0x100000001b3);
        hash_b ^= u64::from(byte).wrapping_add(0x9e);
        hash_b = hash_b.rotate_left(7).wrapping_mul(0x9e3779b185ebca87);
    }
    Ok(format!("{hash_a:016x}{hash_b:016x}"))
}

#[cfg(test)]
mod tests {
    use super::definitions_hash;
    use crate::nl_dsl::parse_definitions;

    #[test]
    fn hash_is_deterministic_for_equivalent_definitions() {
        let first = parse_definitions("name: Example\ninputs: []\ndecisions: []\n").unwrap();
        let second = parse_definitions("name: Example\ninputs: []\ndecisions: []\n").unwrap();
        assert_eq!(
            definitions_hash(&first).unwrap(),
            definitions_hash(&second).unwrap()
        );
    }
}
