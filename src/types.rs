use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) db: PgPool,
    pub(crate) jwt_secret: String,
    pub(crate) hcaptcha_secret: String,
    pub(crate) hcaptcha_site_key: String,
    pub(crate) captchas: Arc<Mutex<HashMap<String, CaptchaChallenge>>>,
    pub(crate) captcha_rate_limits: Arc<Mutex<HashMap<String, RateLimitWindow>>>,
    pub(crate) login_rate_limits: Arc<Mutex<HashMap<String, RateLimitWindow>>>,
    pub(crate) captcha_verify_limits: Arc<Mutex<HashMap<String, RateLimitWindow>>>,
    pub(crate) trust_proxy_headers: bool,
    pub(crate) boot_time: Instant,
}

#[derive(Debug, Clone)]
pub(crate) struct CaptchaChallenge {
    pub answer_digest: [u8; 32],
    pub created_at: Instant,
    pub ttl_seconds: u64,
    pub attempts: u8,
    pub client_binding: [u8; 32],
}

impl CaptchaChallenge {
    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed().as_secs() > self.ttl_seconds
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RateLimitWindow {
    pub created_at: Instant,
    pub count: u16,
}

// ==========================================
// 1. 数据结构
// ==========================================
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String, // username
    pub exp: usize,
}

// 登录请求体（出现在 pub(crate) handler 签名中，需为 crate 可见）
#[derive(Debug, Deserialize)]
pub(crate) struct AuthPayload {
    pub username: String,
    pub password: String,
    #[serde(rename = "hcaptchaToken")]
    pub hcaptcha_token: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct LoginResponse {
    pub token: String,
}
