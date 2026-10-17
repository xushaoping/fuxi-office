use axum::{
    Extension, Json,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::{Duration, Utc};
use hmac::{Hmac, Mac};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use sha2::Sha256;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Mutex,
};
use uuid::Uuid;

// 引入加密和数据库模块
use crate::models::{AdminUser, CreateUser, UpdateUser, UserRecord};
use crate::types::{
    AppState, AuthPayload, CaptchaChallenge, Claims, LoginResponse, RateLimitWindow,
};
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use axum::middleware::Next;

// ==========================================
// 2. 密码哈希工具函数
// ==========================================
fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    Ok(argon2
        .hash_password(password.as_bytes(), &salt)?
        .to_string())
}

fn verify_password(hash: &str, password: &str) -> Result<(), argon2::password_hash::Error> {
    let parsed_hash = PasswordHash::new(hash)?;
    Argon2::default().verify_password(password.as_bytes(), &parsed_hash)
}

fn bearer_token(headers: &axum::http::HeaderMap) -> Option<&str> {
    let value = headers.get("Authorization")?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
}

fn database_failure(operation: &str, error: &sqlx::Error) -> (StatusCode, String) {
    eprintln!("数据库操作失败 [{operation}]: {error}");

    let environment = std::env::var("APP_ENV")
        .or_else(|_| std::env::var("RUST_ENV"))
        .or_else(|_| std::env::var("LEPTOS_ENV"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(environment.as_str(), "production" | "prod") {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{operation}失败，请稍后重试"),
        )
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{operation}失败：{error}"),
        )
    }
}

// ==========================================
// 3. API 接口
// ==========================================

const CAPTCHA_ALPHABET: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
const CAPTCHA_TTL_SECONDS: u64 = 120;
const CAPTCHA_LENGTH: usize = 5;
const CAPTCHA_MAX_ATTEMPTS: u8 = 5;
const CAPTCHA_MAX_ENTRIES: usize = 10_000;
const RATE_LIMIT_WINDOW_SECONDS: u64 = 60;
const CAPTCHA_ISSUE_LIMIT: u16 = 20;
const CAPTCHA_VERIFY_LIMIT: u16 = 30;
const LOGIN_LIMIT: u16 = 10;
const RATE_LIMIT_MAX_KEYS: usize = 10_000;
const RATE_LIMIT_RETRY_AFTER_SECONDS: u64 = 60;
const HCAPTCHA_VERIFY_URL: &str = "https://api.hcaptcha.com/siteverify";
type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, serde::Deserialize)]
struct HcaptchaVerificationResponse {
    success: bool,
    #[serde(default)]
    error_codes: Vec<String>,
}

async fn verify_hcaptcha(
    secret: &str,
    site_key: &str,
    token: &str,
    client_key: &str,
) -> Result<bool, String> {
    let mut form = vec![("secret", secret), ("sitekey", site_key), ("response", token)];
    if client_key.parse::<IpAddr>().is_ok() {
        form.push(("remoteip", client_key));
    }
    let response = reqwest::Client::new()
        .post(HCAPTCHA_VERIFY_URL)
        .form(&form)
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await
        .map_err(|error| format!("hCaptcha 校验请求失败：{error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("hCaptcha 校验服务返回 HTTP {}", status.as_u16()));
    }
    let result = response
        .json::<HcaptchaVerificationResponse>()
        .await
        .map_err(|error| format!("hCaptcha 校验响应无效：{error}"))?;
    if !result.success && !result.error_codes.is_empty() {
        eprintln!("hCaptcha 校验未通过：{}", result.error_codes.join(","));
    }
    Ok(result.success)
}

fn captcha_glyph(character: char) -> [&'static str; 7] {
    match character {
        '2' => [
            "11110", "00001", "00010", "00100", "01000", "10000", "11111",
        ],
        '3' => [
            "11110", "00001", "00110", "00001", "00001", "00001", "11110",
        ],
        '4' => [
            "00010", "00110", "01010", "10010", "11111", "00010", "00010",
        ],
        '5' => [
            "11111", "10000", "11110", "00001", "00001", "10001", "01110",
        ],
        '6' => [
            "00111", "01000", "10000", "11110", "10001", "10001", "01110",
        ],
        '7' => [
            "11111", "00001", "00010", "00100", "01000", "01000", "01000",
        ],
        '8' => [
            "01110", "10001", "10001", "01110", "10001", "10001", "01110",
        ],
        '9' => [
            "01110", "10001", "10001", "01111", "00001", "00010", "11100",
        ],
        'A' => [
            "01110", "10001", "10001", "11111", "10001", "10001", "10001",
        ],
        'B' => [
            "11110", "10001", "10001", "11110", "10001", "10001", "11110",
        ],
        'C' => [
            "01111", "10000", "10000", "10000", "10000", "10000", "01111",
        ],
        'D' => [
            "11110", "10001", "10001", "10001", "10001", "10001", "11110",
        ],
        'E' => [
            "11111", "10000", "10000", "11110", "10000", "10000", "11111",
        ],
        'F' => [
            "11111", "10000", "10000", "11110", "10000", "10000", "10000",
        ],
        'G' => [
            "01111", "10000", "10000", "10111", "10001", "10001", "01111",
        ],
        'H' => [
            "10001", "10001", "10001", "11111", "10001", "10001", "10001",
        ],
        'J' => [
            "00111", "00010", "00010", "00010", "10010", "10010", "01100",
        ],
        'K' => [
            "10001", "10010", "10100", "11000", "10100", "10010", "10001",
        ],
        'L' => [
            "10000", "10000", "10000", "10000", "10000", "10000", "11111",
        ],
        'M' => [
            "10001", "11011", "10101", "10101", "10001", "10001", "10001",
        ],
        'N' => [
            "10001", "11001", "10101", "10011", "10001", "10001", "10001",
        ],
        'P' => [
            "11110", "10001", "10001", "11110", "10000", "10000", "10000",
        ],
        'Q' => [
            "01110", "10001", "10001", "10001", "10101", "10010", "01101",
        ],
        'R' => [
            "11110", "10001", "10001", "11110", "10100", "10010", "10001",
        ],
        'S' => [
            "01111", "10000", "10000", "01110", "00001", "00001", "11110",
        ],
        'T' => [
            "11111", "00100", "00100", "00100", "00100", "00100", "00100",
        ],
        'U' => [
            "10001", "10001", "10001", "10001", "10001", "10001", "01110",
        ],
        'V' => [
            "10001", "10001", "10001", "10001", "10001", "01010", "00100",
        ],
        'W' => [
            "10001", "10001", "10001", "10101", "10101", "11011", "10001",
        ],
        'X' => [
            "10001", "10001", "01010", "00100", "01010", "10001", "10001",
        ],
        'Y' => [
            "10001", "10001", "01010", "00100", "00100", "00100", "00100",
        ],
        'Z' => [
            "11111", "00001", "00010", "00100", "01000", "10000", "11111",
        ],
        _ => [
            "11111", "10001", "10101", "10001", "10101", "10001", "11111",
        ],
    }
}

fn hmac_digest(secret: &str, domain: &[u8], value: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC-SHA256 accepts keys of any length");
    mac.update(domain);
    mac.update(value);
    mac.finalize().into_bytes().into()
}

fn captcha_answer_digest(secret: &str, captcha_id: &str, captcha_code: &str) -> [u8; 32] {
    let normalized_code = captcha_code.trim().to_ascii_uppercase();
    let mut value = Vec::with_capacity(captcha_id.len() + normalized_code.len() + 1);
    value.extend_from_slice(captcha_id.as_bytes());
    value.push(b':');
    value.extend_from_slice(normalized_code.as_bytes());
    hmac_digest(secret, b"fuxi-dmn:captcha-answer:v1:", &value)
}

fn captcha_client_binding(secret: &str, client_key: &str) -> [u8; 32] {
    hmac_digest(
        secret,
        b"fuxi-dmn:captcha-client:v1:",
        client_key.as_bytes(),
    )
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == 0
}

fn forwarded_client_ip(headers: &HeaderMap) -> Option<IpAddr> {
    for header_name in ["x-forwarded-for", "x-real-ip"] {
        let Some(value) = headers
            .get(header_name)
            .and_then(|value| value.to_str().ok())
        else {
            continue;
        };
        if let Some(ip) = value
            .split(',')
            .find_map(|candidate| candidate.trim().parse::<IpAddr>().ok())
        {
            return Some(ip);
        }
    }
    None
}

fn client_key(
    headers: &HeaderMap,
    peer_addr: Option<SocketAddr>,
    trust_proxy_headers: bool,
) -> String {
    let ip = if trust_proxy_headers {
        forwarded_client_ip(headers).or_else(|| peer_addr.map(|address| address.ip()))
    } else {
        peer_addr.map(|address| address.ip())
    };
    ip.map_or_else(|| "unknown".to_string(), |ip| ip.to_string())
}

fn allow_rate_limit(
    limits: &Mutex<HashMap<String, RateLimitWindow>>,
    key: &str,
    max_requests: u16,
) -> bool {
    let Ok(mut limits) = limits.lock() else {
        return false;
    };

    // 清理过期窗口
    limits.retain(|_, window| window.created_at.elapsed().as_secs() <= RATE_LIMIT_WINDOW_SECONDS);

    if let Some(window) = limits.get_mut(key) {
        if window.count >= max_requests {
            return false;
        }
        window.count = window.count.saturating_add(1);
        return true;
    }

    if limits.len() >= RATE_LIMIT_MAX_KEYS {
        if let Some(oldest_key) = limits
            .iter()
            .min_by_key(|(_, window)| window.created_at)
            .map(|(key, _)| key.clone())
        {
            limits.remove(&oldest_key);
        }
    }

    limits.insert(
        key.to_string(),
        RateLimitWindow {
            created_at: std::time::Instant::now(),
            count: 1,
        },
    );
    true
}

fn no_store_json(
    status: StatusCode,
    body: serde_json::Value,
    retry_after: Option<u64>,
) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, private"),
    );
    headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    if let Some(seconds) = retry_after {
        if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
            headers.insert(header::RETRY_AFTER, value);
        }
    }
    (status, headers, Json(body)).into_response()
}

fn consume_captcha(
    captchas: &Mutex<HashMap<String, CaptchaChallenge>>,
    secret: &str,
    captcha_id: &str,
    captcha_code: &str,
    client_key: &str,
) -> bool {
    let Ok(mut captchas) = captchas.lock() else {
        return false;
    };

    // 一次性取出挑战，避免 TOCTOU 竞态条件
    let Some(mut challenge) = captchas.remove(captcha_id) else {
        return false;
    };

    // 检查是否过期
    if challenge.is_expired() {
        return false; // 已过期，不放回
    }

    let expected_answer = captcha_answer_digest(secret, captcha_id, captcha_code);
    let expected_binding = captcha_client_binding(secret, client_key);

    challenge.attempts = challenge.attempts.saturating_add(1);

    let answer_match = constant_time_eq(&challenge.answer_digest, &expected_answer);
    let binding_match = constant_time_eq(&challenge.client_binding, &expected_binding);

    let valid = challenge.attempts <= CAPTCHA_MAX_ATTEMPTS && answer_match && binding_match;

    // 如果验证失败且未超过最大尝试次数，放回队列
    if !valid && challenge.attempts < CAPTCHA_MAX_ATTEMPTS {
        captchas.insert(captcha_id.to_string(), challenge);
    }

    valid
}

// 登录验证码接口
pub(crate) async fn captcha(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let peer_addr = Some(addr);
    let client_key = client_key(&headers, peer_addr, state.trust_proxy_headers);

    if !allow_rate_limit(&state.captcha_rate_limits, &client_key, CAPTCHA_ISSUE_LIMIT) {
        return no_store_json(
            StatusCode::TOO_MANY_REQUESTS,
            serde_json::json!({"error": "验证码请求过于频繁，请稍后再试"}),
            Some(RATE_LIMIT_RETRY_AFTER_SECONDS),
        );
    }

    let id = Uuid::new_v4().to_string();
    let bytes = *Uuid::new_v4().as_bytes();
    let code: String = (0..CAPTCHA_LENGTH)
        .map(|index| CAPTCHA_ALPHABET[(bytes[index] as usize) % CAPTCHA_ALPHABET.len()] as char)
        .collect();

    let mut glyphs = String::new();
    for (index, character) in code.chars().enumerate() {
        let rotation = (bytes[index + 6] % 13) as i8 - 6;
        let x = 5 + index * 21;
        let mut glyph = String::new();
        for (row, pixels) in captcha_glyph(character).iter().enumerate() {
            for (column, pixel) in pixels.bytes().enumerate() {
                if pixel == b'1' {
                    glyph.push_str(&format!(
                        "<rect x=\"{}\" y=\"{}\" width=\"3\" height=\"3\" rx=\"1\"/>",
                        x + column * 3,
                        10 + row * 3
                    ));
                }
            }
        }
        glyphs.push_str(&format!(
            "<g transform=\"rotate({rotation} {} 22)\">{glyph}</g>",
            x + 7
        ));
    }

    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="140" height="44" viewBox="0 0 140 44"><rect width="140" height="44" rx="8" fill="#eef2f7"/><path d="M8 34L132 9M5 12l125 23M22 4l8 36M82 3l-9 38" stroke="#cbd5e1" stroke-width="1"/><g fill="#25324a">{glyphs}</g></svg>"##
    );
    let image = format!("data:image/svg+xml;base64,{}", BASE64.encode(svg));
    let binding = captcha_client_binding(&state.jwt_secret, &client_key);
    let challenge = CaptchaChallenge {
        answer_digest: captcha_answer_digest(&state.jwt_secret, &id, &code),
        created_at: std::time::Instant::now(),
        ttl_seconds: CAPTCHA_TTL_SECONDS,
        attempts: 0,
        client_binding: binding,
    };

    let Ok(mut captchas) = state.captchas.lock() else {
        return no_store_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({"error": "验证码服务暂不可用，请稍后重试"}),
            None,
        );
    };

    // 清理过期和同客户端的旧验证码
    captchas.retain(|_, existing| !existing.is_expired() && existing.client_binding != binding);

    if captchas.len() >= CAPTCHA_MAX_ENTRIES {
        if let Some(oldest_key) = captchas
            .iter()
            .min_by_key(|(_, existing)| existing.created_at)
            .map(|(key, _)| key.clone())
        {
            captchas.remove(&oldest_key);
        }
    }

    captchas.insert(id.clone(), challenge);

    no_store_json(
        StatusCode::OK,
        serde_json::json!({
            "captchaId": id,
            "image": image,
            "expiresIn": CAPTCHA_TTL_SECONDS,
        }),
        None,
    )
}

pub(crate) async fn hcaptcha_config(State(state): State<AppState>) -> impl IntoResponse {
    no_store_json(
        StatusCode::OK,
        serde_json::json!({"siteKey": state.hcaptcha_site_key}),
        None,
    )
}

pub(crate) async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<AuthPayload>,
) -> impl IntoResponse {
    let peer_addr = Some(addr);
    let client_key = client_key(&headers, peer_addr, state.trust_proxy_headers);

    // 登录频率限制
    if !allow_rate_limit(&state.login_rate_limits, &client_key, LOGIN_LIMIT) {
        eprintln!(
            "[安全警告] 登录频率超限 - 客户端: {}, 用户名: {}",
            client_key, payload.username
        );
        return no_store_json(
            StatusCode::TOO_MANY_REQUESTS,
            serde_json::json!({"error": "登录尝试过于频繁，请稍后再试"}),
            Some(RATE_LIMIT_RETRY_AFTER_SECONDS),
        );
    }

    // 验证码验证频率限制
    if !allow_rate_limit(
        &state.captcha_verify_limits,
        &client_key,
        CAPTCHA_VERIFY_LIMIT,
    ) {
        eprintln!(
            "[安全警告] 验证码验证频率超限 - 客户端: {}, 用户名: {}",
            client_key, payload.username
        );
        return no_store_json(
            StatusCode::TOO_MANY_REQUESTS,
            serde_json::json!({"error": "验证码验证过于频繁，请稍后再试"}),
            Some(RATE_LIMIT_RETRY_AFTER_SECONDS),
        );
    }

    let hcaptcha_token = payload.hcaptcha_token.trim();
    if hcaptcha_token.is_empty() {
        eprintln!(
            "[审计] hCaptcha token 缺失 - 客户端: {}, 用户名: {}",
            client_key, payload.username
        );
        return no_store_json(
            StatusCode::BAD_REQUEST,
            serde_json::json!({"error": "请先完成 hCaptcha 验证"}),
            None,
        );
    }

    if hcaptcha_token.len() > 4096 {
        return no_store_json(
            StatusCode::BAD_REQUEST,
            serde_json::json!({"error": "hCaptcha 验证信息无效，请重试"}),
            None,
        );
    }
    let valid_hcaptcha = match verify_hcaptcha(
        &state.hcaptcha_secret,
        &state.hcaptcha_site_key,
        hcaptcha_token,
        &client_key,
    )
    .await
    {
        Ok(valid) => valid,
        Err(error) => {
            eprintln!("[错误] hCaptcha 校验失败 - 客户端: {}, 错误: {}", client_key, error);
            return no_store_json(
                StatusCode::SERVICE_UNAVAILABLE,
                serde_json::json!({"error": "验证码服务暂不可用，请稍后重试"}),
                None,
            );
        }
    };
    if !valid_hcaptcha {
        eprintln!(
            "[审计] hCaptcha 验证失败 - 客户端: {}, 用户名: {}",
            client_key, payload.username
        );
        return no_store_json(
            StatusCode::BAD_REQUEST,
            serde_json::json!({"error": "hCaptcha 验证未通过，请重试"}),
            None,
        );
    }

    // 1. 从数据库查询用户
    let username = payload.username.trim();
    let user = match sqlx::query_as::<_, UserRecord>(
        "SELECT username, password_hash, role FROM users WHERE username = $1",
    )
    .bind(username)
    .fetch_optional(&state.db)
    .await
    {
        Ok(Some(user)) => user,
        Ok(None) => {
            eprintln!(
                "[审计] 登录失败 - 用户不存在 - 客户端: {}, 用户名: {}",
                client_key, username
            );
            return no_store_json(
                StatusCode::UNAUTHORIZED,
                serde_json::json!({"error": "用户名、密码或验证码不正确"}),
                None,
            );
        }
        Err(error) => {
            eprintln!(
                "[错误] 登录数据库查询失败 - 客户端: {}, 用户名: {}, 错误: {}",
                client_key, username, error
            );
            return no_store_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": "登录服务暂不可用，请稍后重试"}),
                None,
            );
        }
    };

    // 2. 校验密码
    if let Err(_) = verify_password(&user.password_hash, &payload.password) {
        eprintln!(
            "[审计] 登录失败 - 密码错误 - 客户端: {}, 用户名: {}",
            client_key, username
        );
        return no_store_json(
            StatusCode::UNAUTHORIZED,
            serde_json::json!({"error": "用户名、密码或验证码不正确"}),
            None,
        );
    }

    // 3. 签发 JWT
    let exp = (Utc::now() + Duration::hours(24)).timestamp() as usize;
    let claims = Claims {
        sub: user.username.clone(),
        exp,
    };

    let token = match encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.jwt_secret.as_bytes()),
    ) {
        Ok(token) => token,
        Err(error) => {
            eprintln!(
                "[错误] JWT 签发失败 - 客户端: {}, 用户名: {}, 错误: {}",
                client_key, username, error
            );
            return no_store_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": "登录服务暂不可用，请稍后重试"}),
                None,
            );
        }
    };

    println!(
        "[审计] 登录成功 - 客户端: {}, 用户名: {}, 角色: {}",
        client_key, user.username, user.role
    );

    Json(LoginResponse { token }).into_response()
}

// 获取当前用户接口
pub(crate) async fn get_me(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let role = sqlx::query_scalar::<_, String>("SELECT role FROM users WHERE username = $1")
        .bind(&claims.sub)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .unwrap_or_else(|| "user".to_string());
    Ok(Json(
        serde_json::json!({ "username": claims.sub, "role": role }),
    ))
}

pub(crate) async fn ensure_admin_user(db: &sqlx::PgPool) -> Result<(), sqlx::Error> {
    let password = std::env::var("ADMIN_PASSWORD")
        .map_err(|_| sqlx::Error::Protocol("ADMIN_PASSWORD 必须设置且不能使用默认值".into()))?;
    if password.trim().is_empty()
        || password == "change_this_admin_password"
        || password.len() < 6
        || password.len() > 1024
    {
        return Err(sqlx::Error::Protocol(
            "ADMIN_PASSWORD 长度必须为 6-1024 字节".into(),
        ));
    }
    let hash = hash_password(&password)
        .map_err(|error| sqlx::Error::Protocol(format!("admin 密码加密失败: {error}").into()))?;
    sqlx::query(
        "INSERT INTO users (username, password_hash, role) VALUES ('admin', $1, 'admin')
         ON CONFLICT (username) DO UPDATE SET role = 'admin'",
    )
    .bind(hash)
    .execute(db)
    .await
    .map(|_| ())
}

async fn require_admin(state: &AppState, claims: &Claims) -> Result<(), (StatusCode, String)> {
    let is_admin = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM users WHERE username = $1 AND role = 'admin')",
    )
    .bind(&claims.sub)
    .fetch_one(&state.db)
    .await
    .map_err(|error| database_failure("验证管理员权限", &error))?;
    if is_admin {
        Ok(())
    } else {
        Err((StatusCode::FORBIDDEN, "仅超级管理员可管理用户".to_string()))
    }
}

fn validate_user_payload(
    username: &str,
    password: &str,
    role: &str,
) -> Result<(), (StatusCode, String)> {
    if username.trim().is_empty() || username.chars().count() > 255 {
        return Err((
            StatusCode::BAD_REQUEST,
            "用户名不能为空且不超过 255 字符".to_string(),
        ));
    }
    if password.len() < 6 || password.len() > 1024 {
        return Err((
            StatusCode::BAD_REQUEST,
            "密码长度必须为 6-1024 字节".to_string(),
        ));
    }
    if !matches!(role, "admin" | "user") {
        return Err((
            StatusCode::BAD_REQUEST,
            "用户角色只能是 admin 或 user".to_string(),
        ));
    }
    Ok(())
}

pub(crate) async fn list_users(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<AdminUser>>, (StatusCode, String)> {
    require_admin(&state, &claims).await?;
    let users = sqlx::query_as::<_, AdminUser>(
        "SELECT username, role, created_at FROM users ORDER BY username",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|error| database_failure("读取用户列表", &error))?;
    Ok(Json(users))
}

pub(crate) async fn create_user(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<CreateUser>,
) -> Result<(StatusCode, Json<AdminUser>), (StatusCode, String)> {
    require_admin(&state, &claims).await?;
    let username = payload.username.trim();
    validate_user_payload(username, &payload.password, &payload.role)?;
    let hash = hash_password(&payload.password).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "密码加密失败".to_string(),
        )
    })?;
    let user = sqlx::query_as::<_, AdminUser>(
        "INSERT INTO users (username, password_hash, role) VALUES ($1, $2, $3)
         RETURNING username, role, created_at",
    )
    .bind(username)
    .bind(hash)
    .bind(&payload.role)
    .fetch_one(&state.db)
    .await
    .map_err(|error| {
        if error
            .as_database_error()
            .is_some_and(|db| db.is_unique_violation())
        {
            (StatusCode::CONFLICT, "用户名已存在".to_string())
        } else {
            database_failure("添加用户", &error)
        }
    })?;
    Ok((StatusCode::CREATED, Json(user)))
}

pub(crate) async fn update_user(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(username): Path<String>,
    Json(payload): Json<UpdateUser>,
) -> Result<Json<AdminUser>, (StatusCode, String)> {
    require_admin(&state, &claims).await?;
    let username = username.trim();
    if !matches!(payload.role.as_str(), "admin" | "user") {
        return Err((
            StatusCode::BAD_REQUEST,
            "用户角色只能是 admin 或 user".to_string(),
        ));
    }
    if username == "admin" && payload.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            "不能取消 admin 的超级管理员角色".to_string(),
        ));
    }
    if let Some(password) = &payload.password {
        if password.len() < 6 || password.len() > 1024 {
            return Err((
                StatusCode::BAD_REQUEST,
                "密码长度必须为 6-1024 字节".to_string(),
            ));
        }
    }
    let hash = payload
        .password
        .as_deref()
        .map(hash_password)
        .transpose()
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "密码加密失败".to_string(),
            )
        })?;
    let user = sqlx::query_as::<_, AdminUser>(
        "UPDATE users SET role = $2, password_hash = COALESCE($3, password_hash)
         WHERE username = $1 RETURNING username, role, created_at",
    )
    .bind(username)
    .bind(&payload.role)
    .bind(hash)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| database_failure("更新用户", &error))?
    .ok_or((StatusCode::NOT_FOUND, "用户不存在".to_string()))?;
    Ok(Json(user))
}

pub(crate) async fn delete_user(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(username): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    require_admin(&state, &claims).await?;
    let username = username.trim();
    if username == "admin" {
        return Err((StatusCode::FORBIDDEN, "不能删除 admin 用户".to_string()));
    }
    let deleted =
        sqlx::query_scalar::<_, bool>("DELETE FROM users WHERE username = $1 RETURNING true")
            .bind(username)
            .fetch_optional(&state.db)
            .await
            .map_err(|error| database_failure("删除用户", &error))?
            .is_some();
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "用户不存在".to_string()))
    }
}

// ==========================================
// 4. 鉴权中间件
// ==========================================
pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let token = bearer_token(req.headers()).ok_or(StatusCode::UNAUTHORIZED)?;

    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| StatusCode::UNAUTHORIZED)?;

    // 将解析出的用户信息塞入请求上下文，后续业务代码直接提取
    req.extensions_mut().insert(token_data.claims);

    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_scheme_is_case_insensitive_and_rejects_empty_tokens() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("Authorization", "bearer token-value".parse().unwrap());
        assert_eq!(bearer_token(&headers), Some("token-value"));

        headers.insert("Authorization", "Bearer   ".parse().unwrap());
        assert_eq!(bearer_token(&headers), None);
    }

    #[test]
    fn captcha_is_single_use_and_case_insensitive() {
        let store = std::sync::Mutex::new(std::collections::HashMap::from([(
            "challenge".to_string(),
            CaptchaChallenge {
                answer_digest: captcha_answer_digest("test-secret", "challenge", "Ab3K9"),
                created_at: std::time::Instant::now(),
                ttl_seconds: 120,
                attempts: 0,
                client_binding: captcha_client_binding("test-secret", "test-client"),
            },
        )]));
        assert!(consume_captcha(
            &store,
            "test-secret",
            "challenge",
            " ab3k9 ",
            "test-client"
        ));
        assert!(!consume_captcha(
            &store,
            "test-secret",
            "challenge",
            "AB3K9",
            "test-client"
        ));
    }

    #[test]
    fn expired_captcha_is_rejected() {
        let store = std::sync::Mutex::new(std::collections::HashMap::from([(
            "expired".to_string(),
            CaptchaChallenge {
                answer_digest: captcha_answer_digest("test-secret", "expired", "AB123"),
                created_at: std::time::Instant::now() - std::time::Duration::from_secs(121),
                ttl_seconds: 120,
                attempts: 0,
                client_binding: captcha_client_binding("test-secret", "test-client"),
            },
        )]));
        assert!(!consume_captcha(
            &store,
            "test-secret",
            "expired",
            "AB123",
            "test-client"
        ));
    }

    #[test]
    fn captcha_bound_to_client() {
        let store = std::sync::Mutex::new(std::collections::HashMap::from([(
            "challenge".to_string(),
            CaptchaChallenge {
                answer_digest: captcha_answer_digest("test-secret", "challenge", "ABC12"),
                created_at: std::time::Instant::now(),
                ttl_seconds: 120,
                attempts: 0,
                client_binding: captcha_client_binding("test-secret", "client-1"),
            },
        )]));
        // 不同客户端尝试使用验证码应该失败
        assert!(!consume_captcha(
            &store,
            "test-secret",
            "challenge",
            "ABC12",
            "client-2"
        ));
        // 正确的客户端应该成功
        assert!(consume_captcha(
            &store,
            "test-secret",
            "challenge",
            "ABC12",
            "client-1"
        ));
    }
}
