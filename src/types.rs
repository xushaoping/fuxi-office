use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) db: PgPool,
    pub(crate) jwt_secret: String,
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
}

#[derive(Debug, Serialize)]
pub(crate) struct LoginResponse {
    pub token: String,
}
