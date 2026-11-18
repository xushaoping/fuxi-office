const EMBEDDING_MAX_ATTEMPTS: u32 = 3;
const DEFAULT_EMBEDDING_DIMENSIONS: usize = 1024;
const LOCAL_EMBEDDING_MODEL: &str = "local-feature-hash-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmbeddingProvider {
    OpenAiCompatible,
    Local,
}

#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    pub enabled: bool,
    pub model: String,
    pub api_base: String,
    pub api_key: Option<String>,
    pub dimensions: usize,
    provider: EmbeddingProvider,
}

impl EmbeddingConfig {
    pub fn min_vector_similarity(&self) -> f64 {
        match self.provider {
            EmbeddingProvider::Local => 0.1,
            EmbeddingProvider::OpenAiCompatible => 0.5,
        }
    }

    pub fn from_env() -> Self {
        Self::from_values(
            std::env::var("FUXI_EMBEDDING_ENABLED")
                .ok()
                .or_else(|| std::env::var("EMBEDDING_ENABLED").ok())
                .as_deref(),
            std::env::var("FUXI_EMBEDDING_PROVIDER")
                .ok()
                .or_else(|| std::env::var("EMBEDDING_PROVIDER").ok())
                .as_deref(),
            std::env::var("OPENAI_EMBEDDING_MODEL").ok(),
            std::env::var("OPENAI_API_BASE")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| std::env::var("OPENAI_BASE_URL").ok()),
            std::env::var("OPENAI_API_KEY").ok(),
        )
    }

    pub fn http_client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    }

    fn from_values(
        enabled: Option<&str>,
        provider: Option<&str>,
        model: Option<String>,
        api_base: Option<String>,
        api_key: Option<String>,
    ) -> Self {
        let enabled = enabled
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false);
        let provider = match provider
            .unwrap_or("openai-compatible")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "local" => EmbeddingProvider::Local,
            _ => EmbeddingProvider::OpenAiCompatible,
        };
        let model = match provider {
            EmbeddingProvider::Local => LOCAL_EMBEDDING_MODEL.into(),
            EmbeddingProvider::OpenAiCompatible => {
                model.unwrap_or_else(|| "text-embedding-3-large".into())
            }
        };
        Self {
            enabled,
            model,
            api_base: api_base
                .unwrap_or_else(|| "https://api.openai.com/v1".into())
                .trim_end_matches('/')
                .to_string(),
            api_key: api_key.filter(|key| !key.trim().is_empty()),
            dimensions: DEFAULT_EMBEDDING_DIMENSIONS,
            provider,
        }
    }

    pub async fn embed_texts(
        &self,
        client: &reqwest::Client,
        texts: &[String],
    ) -> Result<Option<Vec<Vec<f32>>>, String> {
        if !self.enabled || texts.is_empty() {
            return Ok(None);
        }
        if self.provider == EmbeddingProvider::Local {
            return Ok(Some(
                texts
                    .iter()
                    .map(|text| super::local_embedding::embed(text, self.dimensions))
                    .collect(),
            ));
        }
        let Some(api_key) = self.api_key.as_deref() else {
            return Err("EMBEDDING_ENABLED=true 但未配置 OPENAI_API_KEY".into());
        };
        let endpoint = format!("{}/embeddings", self.api_base);
        let request = serde_json::json!({
            "model": self.model,
            "input": texts,
            "dimensions": self.dimensions,
            "encoding_format": "float",
        });
        let mut response_data = None;
        for attempt in 1..=EMBEDDING_MAX_ATTEMPTS {
            let response = client
                .post(&endpoint)
                .bearer_auth(api_key)
                .json(&request)
                .send()
                .await;
            let response = match response {
                Ok(response) => response,
                Err(error)
                    if attempt < EMBEDDING_MAX_ATTEMPTS
                        && (error.is_timeout() || error.is_connect()) =>
                {
                    tokio::time::sleep(embedding_retry_delay(attempt)).await;
                    continue;
                }
                Err(error) => return Err(format!("Embedding API 请求失败：{error}")),
            };
            if !response.status().is_success() {
                let status = response.status();
                if attempt < EMBEDDING_MAX_ATTEMPTS && retryable_embedding_status(status) {
                    tokio::time::sleep(embedding_retry_delay(attempt)).await;
                    continue;
                }
                return Err(format!("Embedding API 返回 HTTP {status}"));
            }
            response_data = Some(
                response
                    .json::<EmbeddingResponse>()
                    .await
                    .map_err(|error| format!("Embedding API 响应无效：{error}"))?
                    .data,
            );
            break;
        }
        let mut data = response_data.ok_or_else(|| "Embedding API 重试后仍无响应".to_string())?;
        data.sort_by_key(|item| item.index);
        if data.len() != texts.len() {
            return Err(format!(
                "Embedding API 返回 {} 个向量，预期 {} 个",
                data.len(),
                texts.len()
            ));
        }
        if data
            .iter()
            .enumerate()
            .any(|(expected, item)| item.index != expected)
        {
            return Err("Embedding API 返回了重复或不连续的向量序号".into());
        }
        if data.iter().any(|item| {
            item.embedding.len() != self.dimensions
                || item.embedding.iter().any(|value| !value.is_finite())
        }) {
            return Err(format!(
                "Embedding API 向量维度或数值无效，预期维度 {}",
                self.dimensions
            ));
        }
        Ok(Some(data.drain(..).map(|item| item.embedding).collect()))
    }
}

fn retryable_embedding_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::REQUEST_TIMEOUT
        || status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

fn embedding_retry_delay(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_millis(
        250_u64.saturating_mul(1_u64 << attempt.saturating_sub(1).min(4)),
    )
}

#[derive(serde::Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingItem>,
}

#[derive(serde::Deserialize)]
struct EmbeddingItem {
    index: usize,
    embedding: Vec<f32>,
}

#[cfg(test)]
mod tests {
    use super::{
        EmbeddingConfig, EmbeddingProvider, embedding_retry_delay, retryable_embedding_status,
    };

    #[test]
    fn config_defaults_to_keyword_mode_and_openai_embedding_defaults() {
        let config = EmbeddingConfig::from_values(None, None, None, None, None);
        assert!(!config.enabled);
        assert_eq!(config.dimensions, 1024);
        assert_eq!(config.model, "text-embedding-3-large");
        assert_eq!(config.api_base, "https://api.openai.com/v1");
        assert!(config.api_key.is_none());
        assert_eq!(config.provider, EmbeddingProvider::OpenAiCompatible);
    }

    #[test]
    fn local_provider_uses_a_stable_model_name_without_an_api_key() {
        let config = EmbeddingConfig::from_values(Some("true"), Some("local"), None, None, None);
        assert!(config.enabled);
        assert_eq!(config.model, "local-feature-hash-v1");
        assert!(config.api_key.is_none());
        assert_eq!(config.provider, EmbeddingProvider::Local);
        assert_eq!(config.min_vector_similarity(), 0.1);
    }

    #[test]
    fn api_embedding_provider_keeps_the_semantic_similarity_threshold() {
        let config = EmbeddingConfig::from_values(Some("true"), None, None, None, None);
        assert_eq!(config.min_vector_similarity(), 0.5);
    }

    #[tokio::test]
    async fn local_provider_embeds_a_batch_without_network_access() {
        let config = EmbeddingConfig::from_values(Some("true"), Some("local"), None, None, None);
        let vectors = config
            .embed_texts(
                &EmbeddingConfig::http_client(),
                &["订单金额审批".into(), "库存数量".into()],
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(vectors.len(), 2);
        assert!(
            vectors
                .iter()
                .all(|vector| vector.len() == config.dimensions)
        );
        assert!(
            vectors
                .iter()
                .all(|vector| vector.iter().all(|value| value.is_finite()))
        );
    }

    #[test]
    fn embedding_retries_only_transient_http_failures_with_bounded_backoff() {
        for status in [
            reqwest::StatusCode::REQUEST_TIMEOUT,
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
        ] {
            assert!(retryable_embedding_status(status));
        }
        for status in [
            reqwest::StatusCode::BAD_REQUEST,
            reqwest::StatusCode::UNAUTHORIZED,
            reqwest::StatusCode::FORBIDDEN,
        ] {
            assert!(!retryable_embedding_status(status));
        }
        assert_eq!(embedding_retry_delay(1).as_millis(), 250);
        assert_eq!(embedding_retry_delay(2).as_millis(), 500);
        assert_eq!(embedding_retry_delay(100).as_millis(), 4_000);
    }
}
