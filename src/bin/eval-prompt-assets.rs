use fuxi_dmn::nl_dsl::prompts::PromptCatalog;
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PromptAssetReport {
    key: String,
    version: String,
    content_digest: String,
    content_bytes: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PromptReport {
    schema_version: u32,
    evaluation_mode: &'static str,
    llm_calls: bool,
    assets: Vec<PromptAssetReport>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = PromptCatalog::builtins();
    let report = PromptReport {
        schema_version: 1,
        evaluation_mode: "offline_code_contract",
        llm_calls: false,
        assets: catalog
            .assets()
            .into_iter()
            .map(|asset| PromptAssetReport {
                key: asset.key.clone(),
                version: asset.version.clone(),
                content_digest: format!("{:x}", Sha256::digest(asset.content.as_bytes())),
                content_bytes: asset.content.len(),
            })
            .collect(),
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
