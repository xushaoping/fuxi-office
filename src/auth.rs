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
use sha2::{Digest, Sha256};
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

const CAPTCHA_TTL_SECONDS: u64 = 120;
const CAPTCHA_ANSWER_MAX_LENGTH: usize = 3;
const CAPTCHA_WIDTH: usize = 140;
const CAPTCHA_HEIGHT: usize = 44;
const CAPTCHA_MAX_ATTEMPTS: u8 = 5;
const CAPTCHA_MAX_ENTRIES: usize = 10_000;
const RATE_LIMIT_WINDOW_SECONDS: u64 = 60;
const CAPTCHA_ISSUE_LIMIT: u16 = 20;
const CAPTCHA_VERIFY_LIMIT: u16 = 30;
const LOGIN_LIMIT: u16 = 10;
const RATE_LIMIT_MAX_KEYS: usize = 10_000;
const RATE_LIMIT_RETRY_AFTER_SECONDS: u64 = 60;
type HmacSha256 = Hmac<Sha256>;

fn captcha_glyph(character: char) -> [&'static str; 7] {
    match character {
        '0' => [
            "01110", "10001", "10011", "10101", "11001", "10001", "01110",
        ],
        '1' => [
            "00100", "01100", "00100", "00100", "00100", "00100", "01110",
        ],
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
        'X' => [
            "10001", "10001", "01010", "00100", "01010", "10001", "10001",
        ],
        '+' => [
            "00000", "00100", "00100", "11111", "00100", "00100", "00000",
        ],
        '-' => [
            "00000", "00000", "00000", "11111", "00000", "00000", "00000",
        ],
        '=' => [
            "00000", "11111", "00000", "11111", "00000", "00000", "00000",
        ],
        '?' => [
            "01110", "10001", "00001", "00010", "00100", "00000", "00100",
        ],
        _ => [
            "00000", "00000", "00000", "00000", "00000", "00000", "00000",
        ],
    }
}

fn random_byte() -> u8 {
    Uuid::new_v4().as_bytes()[0]
}

fn random_index(max_exclusive: usize) -> usize {
    debug_assert!(max_exclusive > 0 && max_exclusive <= 256);
    let limit = 256 - (256 % max_exclusive);
    loop {
        let value = random_byte() as usize;
        if value < limit {
            return value % max_exclusive;
        }
    }
}

fn generate_captcha_expression() -> (String, String) {
    let mut left = random_index(9) + 1;
    let mut right = random_index(9) + 1;
    let operator = match random_index(3) {
        0 => '+',
        1 => '-',
        _ => 'X',
    };

    if operator == '-' && left < right {
        std::mem::swap(&mut left, &mut right);
    }

    let answer = match operator {
        '+' => left + right,
        '-' => left - right,
        'X' => left * right,
        _ => unreachable!(),
    };
    (format!("{left}{operator}{right}=?"), answer.to_string())
}

fn captcha_answer_is_well_formed(answer: &str) -> bool {
    let answer = answer.trim();
    !answer.is_empty()
        && answer.len() <= CAPTCHA_ANSWER_MAX_LENGTH
        && answer.bytes().all(|character| character.is_ascii_digit())
        && answer.parse::<u16>().is_ok()
}

fn captcha_svg(code: &str) -> String {
    const GLYPH_COLORS: [&str; 5] = ["#25324a", "#304b68", "#28445d", "#344b62", "#263b56"];
    const NOISE_COLORS: [&str; 3] = ["#9aaabd", "#b0bdcb", "#879bb1"];

    let mut noise = String::new();
    for _ in 0..28 {
        let x = 2 + random_index(CAPTCHA_WIDTH - 4);
        let y = 2 + random_index(CAPTCHA_HEIGHT - 4);
        let radius = 1 + random_index(2);
        let color = NOISE_COLORS[random_index(NOISE_COLORS.len())];
        let opacity = 0.28 + random_index(18) as f32 / 100.0;
        noise.push_str(&format!(
            r##"<circle cx="{x}" cy="{y}" r="{radius}" fill="{color}" opacity="{opacity:.2}"/>"##
        ));
    }
    for _ in 0..5 {
        let x1 = random_index(CAPTCHA_WIDTH);
        let y1 = random_index(CAPTCHA_HEIGHT);
        let x2 = random_index(CAPTCHA_WIDTH);
        let y2 = random_index(CAPTCHA_HEIGHT);
        let color = NOISE_COLORS[random_index(NOISE_COLORS.len())];
        let opacity = 0.28 + random_index(18) as f32 / 100.0;
        noise.push_str(&format!(
            r##"<path d="M{x1} {y1}L{x2} {y2}" stroke="{color}" stroke-width="1" opacity="{opacity:.2}"/>"##
        ));
    }
    for _ in 0..2 {
        let start_y = random_index(CAPTCHA_HEIGHT);
        let control_y1 = random_index(CAPTCHA_HEIGHT);
        let control_y2 = random_index(CAPTCHA_HEIGHT);
        let end_y = random_index(CAPTCHA_HEIGHT);
        let color = NOISE_COLORS[random_index(NOISE_COLORS.len())];
        noise.push_str(&format!(
            r##"<path d="M0 {start_y}C35 {control_y1},105 {control_y2},140 {end_y}" fill="none" stroke="{color}" stroke-width="1" stroke-dasharray="6 5" opacity="0.34"/>"##
        ));
    }

    let mut glyphs = String::new();
    for (index, character) in code.chars().enumerate() {
        let x = 5 + index * 26 + random_index(5);
        let y_offset = random_index(7) as i8 - 3;
        let rotation = random_index(13) as i8 - 6;
        let skew = random_index(7) as i8 - 3;
        let color = GLYPH_COLORS[random_index(GLYPH_COLORS.len())];
        let opacity = 0.88 + random_index(10) as f32 / 100.0;
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
            r##"<g transform="translate(0 {y_offset}) rotate({rotation} {pivot_x} 22) skewX({skew})" fill="{color}" opacity="{opacity:.2}">{glyph}</g>"##,
            pivot_x = x + 7,
        ));
    }

    let mut foreground_noise = String::new();
    for _ in 0..2 {
        let start_y = 5 + random_index(CAPTCHA_HEIGHT - 10);
        let control_y1 = random_index(CAPTCHA_HEIGHT);
        let control_y2 = random_index(CAPTCHA_HEIGHT);
        let end_y = 5 + random_index(CAPTCHA_HEIGHT - 10);
        let color = NOISE_COLORS[random_index(NOISE_COLORS.len())];
        foreground_noise.push_str(&format!(
            r##"<path d="M0 {start_y}C35 {control_y1},105 {control_y2},140 {end_y}" fill="none" stroke="{color}" stroke-width="1" stroke-dasharray="3 7" opacity="0.26"/>"##
        ));
    }

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{CAPTCHA_WIDTH}" height="{CAPTCHA_HEIGHT}" viewBox="0 0 {CAPTCHA_WIDTH} {CAPTCHA_HEIGHT}"><rect width="{CAPTCHA_WIDTH}" height="{CAPTCHA_HEIGHT}" rx="8" fill="#eef2f7"/><g>{noise}</g><g>{glyphs}</g><g>{foreground_noise}</g></svg>"##
    )
}

fn hmac_digest(secret: &str, domain: &[u8], value: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC-SHA256 accepts keys of any length");
    mac.update(domain);
    mac.update(value);
    mac.finalize().into_bytes().into()
}

fn captcha_answer_digest(secret: &str, captcha_id: &str, captcha_code: &str) -> [u8; 32] {
    let normalized_code = captcha_code
        .trim()
        .parse::<u16>()
        .map(|answer| answer.to_string())
        .unwrap_or_else(|_| captcha_code.trim().to_ascii_uppercase());
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

fn captcha_work_is_valid(challenge: &[u8; 16], difficulty: u8, nonce: &str) -> bool {
    let Ok(nonce) = nonce.parse::<u64>() else {
        return false;
    };
    if difficulty == 0 || difficulty > 32 {
        return false;
    }

    let mut hasher = Sha256::new();
    hasher.update(challenge);
    hasher.update(nonce.to_be_bytes());
    let digest = hasher.finalize();
    let full_bytes = (difficulty / 8) as usize;
    let remaining_bits = difficulty % 8;

    digest[..full_bytes].iter().all(|byte| *byte == 0)
        && (remaining_bits == 0 || digest[full_bytes] >> (8 - remaining_bits) == 0)
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

fn rate_limit_count(limits: &Mutex<HashMap<String, RateLimitWindow>>, key: &str) -> u16 {
    let Ok(mut limits) = limits.lock() else {
        return 0;
    };

    limits.retain(|_, window| window.created_at.elapsed().as_secs() <= RATE_LIMIT_WINDOW_SECONDS);
    limits.get(key).map_or(0, |window| window.count)
}

fn captcha_pow_difficulty(base: u8, login_attempts: u16) -> u8 {
    let pressure_boost = match login_attempts {
        0..=3 => 0,
        4..=7 => 1,
        _ => 2,
    };
    base.saturating_add(pressure_boost).clamp(8, 20)
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
    captcha_work_nonce: &str,
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

    let expected_binding = captcha_client_binding(secret, client_key);
    if !constant_time_eq(&challenge.client_binding, &expected_binding) {
        // 不同客户端的请求不能消耗原客户端验证码的尝试次数。
        captchas.insert(captcha_id.to_string(), challenge);
        return false;
    }

    if captcha_work_nonce.len() > 20
        || !captcha_work_is_valid(
            &challenge.work_challenge,
            challenge.work_difficulty,
            captcha_work_nonce,
        )
    {
        captchas.insert(captcha_id.to_string(), challenge);
        return false;
    }

    let expected_answer = captcha_answer_digest(secret, captcha_id, captcha_code);
    challenge.attempts = challenge.attempts.saturating_add(1);

    let answer_match = constant_time_eq(&challenge.answer_digest, &expected_answer);
    let valid = challenge.attempts <= CAPTCHA_MAX_ATTEMPTS && answer_match;

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
    let (expression, answer) = generate_captcha_expression();
    let svg = captcha_svg(&expression);
    let image = format!("data:image/svg+xml;base64,{}", BASE64.encode(svg));
    let work_challenge = *Uuid::new_v4().as_bytes();
    let work_difficulty = captcha_pow_difficulty(
        state.captcha_pow_difficulty,
        rate_limit_count(&state.login_rate_limits, &client_key),
    );
    let binding = captcha_client_binding(&state.jwt_secret, &client_key);
    let challenge = CaptchaChallenge {
        answer_digest: captcha_answer_digest(&state.jwt_secret, &id, &answer),
        work_challenge,
        work_difficulty,
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
            "workChallenge": BASE64.encode(work_challenge),
            "workDifficulty": work_difficulty,
        }),
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

    let captcha_id = payload.captcha_id.trim();
    let captcha_code = payload.captcha_code.trim();
    let captcha_work_nonce = payload.captcha_work_nonce.trim();
    if captcha_id.is_empty() || captcha_code.is_empty() || captcha_work_nonce.is_empty() {
        eprintln!(
            "[审计] 本地验证码缺失 - 客户端: {}, 用户名: {}",
            client_key, payload.username
        );
        return no_store_json(
            StatusCode::BAD_REQUEST,
            serde_json::json!({"error": "请先完成验证码验证"}),
            None,
        );
    }

    if captcha_id.len() > 64
        || !captcha_answer_is_well_formed(captcha_code)
        || captcha_work_nonce.len() > 20
    {
        return no_store_json(
            StatusCode::BAD_REQUEST,
            serde_json::json!({"error": "验证码信息无效，请刷新后重试"}),
            None,
        );
    }
    if !consume_captcha(
        &state.captchas,
        &state.jwt_secret,
        captcha_id,
        captcha_code,
        captcha_work_nonce,
        &client_key,
    ) {
        eprintln!(
            "[审计] 本地验证码验证失败 - 客户端: {}, 用户名: {}",
            client_key, payload.username
        );
        return no_store_json(
            StatusCode::BAD_REQUEST,
            serde_json::json!({"error": "验证码不正确或已过期，请刷新后重试"}),
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
    if verify_password(&user.password_hash, &payload.password).is_err() {
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

pub(crate) async fn list_model_performance_metrics(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<crate::models::ModelPerformanceMetricView>>, (StatusCode, String)> {
    require_admin(&state, &claims).await?;
    let metrics = sqlx::query_as::<_, crate::models::ModelPerformanceMetricView>(
        r#"
        SELECT model_name, task_type, sample_count, success_count,
               success_rate::FLOAT8, avg_latency_ms,
               estimated_cost_usd::FLOAT8, last_updated
        FROM model_performance_metrics
        WHERE sample_count > 0
        ORDER BY task_type ASC, success_rate DESC, avg_latency_ms ASC, model_name ASC
        "#,
    )
    .fetch_all(&state.db)
    .await
    .map_err(|error| database_failure("读取模型性能指标", &error))?;
    Ok(Json(metrics))
}

pub(crate) async fn list_decision_explanation_admin_metrics(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<crate::models::DecisionExplanationAdminMetricView>>, (StatusCode, String)> {
    require_admin(&state, &claims).await?;
    let schema_ready = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM pg_attribute WHERE attrelid = to_regclass('public.decision_run_explanations') AND attname = 'generation_revision' AND NOT attisdropped) AND EXISTS (SELECT 1 FROM pg_attribute WHERE attrelid = to_regclass('public.decision_run_explanation_reviews') AND attname = 'explanation_revision' AND NOT attisdropped)",
    )
    .fetch_one(&state.db)
    .await
    .map_err(|error| database_failure("检查决策解释指标表结构", &error))?;
    if !schema_ready {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "决策解释指标所需数据库迁移尚未应用，请按部署文档执行决策解释迁移 092、094、096 和 097 后重试".into(),
        ));
    }
    let metrics = sqlx::query_as::<_, crate::models::DecisionExplanationAdminMetricView>(
        r#"
        WITH recent_explanations AS (
            SELECT id, generation_revision, model, prompt_version, status, created_at, completed_at,
                   GREATEST(
                       (EXTRACT(EPOCH FROM completed_at - created_at) * 1000)::BIGINT,
                       0
                   ) AS latency_ms
            FROM decision_run_explanations
            WHERE created_at >= NOW() - INTERVAL '30 days'
        ), generation_metrics AS (
            SELECT model, prompt_version,
                   COUNT(*)::BIGINT AS total_count,
                   COUNT(*) FILTER (WHERE status = 'succeeded')::BIGINT AS succeeded_count,
                   COUNT(*) FILTER (WHERE status = 'failed')::BIGINT AS failed_count,
                   COUNT(*) FILTER (WHERE status = 'running')::BIGINT AS running_count,
                   COUNT(*) FILTER (
                       WHERE status = 'succeeded' AND completed_at IS NOT NULL
                   )::BIGINT AS latency_sample_count,
                   percentile_disc(0.50) WITHIN GROUP (ORDER BY latency_ms)
                       FILTER (WHERE status = 'succeeded' AND completed_at IS NOT NULL)::BIGINT
                       AS p50_latency_ms,
                   percentile_disc(0.95) WITHIN GROUP (ORDER BY latency_ms)
                       FILTER (WHERE status = 'succeeded' AND completed_at IS NOT NULL)::BIGINT
                       AS p95_latency_ms
            FROM recent_explanations
            GROUP BY model, prompt_version
        ), review_metrics AS (
            SELECT model, prompt_version,
                   COUNT(*)::BIGINT AS review_count,
                   COUNT(DISTINCT explanation_id)::BIGINT AS reviewed_explanation_count,
                   COUNT(*) FILTER (WHERE review_label = 'readable')::BIGINT
                       AS readable_count,
                   COUNT(*) FILTER (WHERE review_label = 'needs_revision')::BIGINT
                       AS needs_revision_count,
                   COUNT(*) FILTER (WHERE review_label = 'inconclusive')::BIGINT
                       AS inconclusive_count
            FROM (
                SELECT explanation.model, explanation.prompt_version, review.explanation_id,
                       review.review_label,
                       review.created_at,
                       ROW_NUMBER() OVER (
                           PARTITION BY review.explanation_id, review.explanation_revision, review.reviewer_username
                           ORDER BY review.created_at, review.id
                       ) AS reviewer_vote
                FROM recent_explanations explanation
                JOIN decision_run_explanation_reviews review
                  ON review.explanation_id = explanation.id
                 AND review.explanation_revision = explanation.generation_revision
                 AND explanation.status = 'succeeded'
            ) first_reviews
            WHERE reviewer_vote = 1
              AND created_at >= NOW() - INTERVAL '30 days'
            GROUP BY model, prompt_version
        )
        SELECT generation.model, generation.prompt_version,
               generation.total_count, generation.succeeded_count,
               generation.failed_count, generation.running_count,
               generation.latency_sample_count, generation.p50_latency_ms,
               generation.p95_latency_ms,
               COALESCE(review.review_count, 0)::BIGINT AS review_count,
               COALESCE(review.reviewed_explanation_count, 0)::BIGINT
                   AS reviewed_explanation_count,
               COALESCE(review.readable_count, 0)::BIGINT AS readable_count,
               COALESCE(review.needs_revision_count, 0)::BIGINT AS needs_revision_count,
               COALESCE(review.inconclusive_count, 0)::BIGINT AS inconclusive_count
        FROM generation_metrics generation
        LEFT JOIN review_metrics review
          ON review.model IS NOT DISTINCT FROM generation.model
         AND review.prompt_version = generation.prompt_version
        ORDER BY generation.prompt_version DESC, generation.model NULLS LAST
        "#,
    )
    .fetch_all(&state.db)
    .await
    .map_err(|error| database_failure("读取决策解释指标", &error))?;
    Ok(Json(metrics))
}

type ExperimentGroupCounts = HashMap<(String, String), (Option<(i64, i64)>, Option<(i64, i64)>)>;

fn experiment_totals_by_group<'a>(
    rows: impl IntoIterator<Item = (&'a str, &'a str, &'a str, i64, i64)>,
) -> ExperimentGroupCounts {
    rows.into_iter().fold(HashMap::new(), |mut totals, row| {
        let (task_type, experiment_name, variant, sample_count, success_count) = row;
        let entry = totals
            .entry((task_type.to_string(), experiment_name.to_string()))
            .or_insert((None, None));
        let counts = (success_count, sample_count);
        match variant {
            "treatment" => entry.0 = Some(counts),
            "control" => entry.1 = Some(counts),
            _ => {}
        }
        totals
    })
}

pub(crate) async fn list_model_experiment_metrics(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<crate::models::ModelExperimentMetricView>>, (StatusCode, String)> {
    require_admin(&state, &claims).await?;
    let rows = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            i64,
            i64,
            f64,
            i64,
            Option<i16>,
            i64,
            i64,
            Option<f64>,
        ),
    >(
        r#"
        WITH latest_project_runs AS (
            SELECT DISTINCT ON (stage, experiment_name, project_id)
                   stage, experiment_name, experiment_variant, project_id, status,
                   started_at, finished_at
            FROM agent_runs
            WHERE experiment_name IS NOT NULL
              AND experiment_variant IS NOT NULL
              AND status IN ('succeeded', 'failed')
              AND finished_at IS NOT NULL
            ORDER BY stage, experiment_name, project_id, started_at DESC, id DESC
        ), experiment_audit AS (
            SELECT stage, experiment_name,
                   MIN(experiment_treatment_percent) AS treatment_percent,
                   COUNT(DISTINCT experiment_treatment_percent)::BIGINT
                       AS treatment_percent_config_count,
                   COUNT(*) FILTER (
                       WHERE experiment_treatment_percent IS NOT NULL
                         AND experiment_bucket IS NOT NULL
                   )::BIGINT AS audit_sample_count,
                   COUNT(*) FILTER (
                       WHERE experiment_treatment_percent IS NOT NULL
                         AND experiment_bucket IS NOT NULL
                         AND ((experiment_bucket < experiment_treatment_percent)
                              = (experiment_variant = 'treatment'))
                   )::BIGINT AS consistent_assignment_count
            FROM agent_runs
            WHERE experiment_name IS NOT NULL
              AND experiment_variant IS NOT NULL
              AND status IN ('succeeded', 'failed')
              AND finished_at IS NOT NULL
            GROUP BY stage, experiment_name
        )
        SELECT stage, experiment_name, experiment_variant AS variant,
               COUNT(*)::BIGINT AS sample_count,
               COUNT(*) FILTER (WHERE status = 'succeeded')::BIGINT AS success_count,
               COALESCE(
                   COUNT(*) FILTER (WHERE status = 'succeeded')::FLOAT8 * 100
                       / NULLIF(COUNT(*), 0),
                   0
               ) AS success_rate,
               COALESCE(
                   AVG(EXTRACT(EPOCH FROM (finished_at - started_at)) * 1000)::BIGINT,
                   0
               ) AS avg_latency_ms,
               audit.treatment_percent,
               audit.treatment_percent_config_count,
               audit.audit_sample_count,
               CASE WHEN audit.audit_sample_count = 0 THEN NULL
                    ELSE audit.consistent_assignment_count::FLOAT8 * 100
                         / audit.audit_sample_count
               END AS assignment_integrity_percent
        FROM latest_project_runs
        JOIN experiment_audit audit USING (stage, experiment_name)
        GROUP BY stage, experiment_name, experiment_variant, audit.treatment_percent,
                 audit.treatment_percent_config_count, audit.audit_sample_count,
                 audit.consistent_assignment_count
        ORDER BY stage, experiment_name, experiment_variant
        "#,
    )
    .fetch_all(&state.db)
    .await
    .map_err(|error| database_failure("读取模型实验指标", &error))?;
    let experiment_totals = experiment_totals_by_group(rows.iter().map(|row| {
        (
            row.0.as_str(),
            row.1.as_str(),
            row.2.as_str(),
            row.3.max(0),
            row.4.max(0),
        )
    }));
    let experiment_effects = experiment_totals
        .into_iter()
        .map(|(group, (treatment, control))| {
            let counts = treatment.zip(control);
            let comparison_sample_size_reached =
                counts.is_some_and(|((_, treatment_size), (_, control_size))| {
                    treatment_size >= crate::agent_runtime::MIN_EXPERIMENT_SAMPLE_SIZE
                        && control_size >= crate::agent_runtime::MIN_EXPERIMENT_SAMPLE_SIZE
                });
            let interval = counts.and_then(
                |((treatment_successes, treatment_size), (control_successes, control_size))| {
                    crate::agent_runtime::wilson_difference_confidence_interval(
                        treatment_successes as u64,
                        treatment_size as u64,
                        control_successes as u64,
                        control_size as u64,
                    )
                },
            );
            let significant = counts.is_some_and(
                |((treatment_successes, treatment_size), (control_successes, control_size))| {
                    crate::agent_runtime::treatment_effect_is_significant(
                        treatment_successes as u64,
                        treatment_size as u64,
                        control_successes as u64,
                        control_size as u64,
                    )
                },
            );
            (
                group,
                (
                    interval.map(|value| value.0 * 100.0),
                    interval.map(|value| value.1 * 100.0),
                    comparison_sample_size_reached,
                    significant,
                ),
            )
        })
        .collect::<HashMap<_, _>>();
    let metrics = rows
        .into_iter()
        .filter_map(
            |(
                task_type,
                experiment_name,
                variant,
                sample_count,
                success_count,
                success_rate,
                avg_latency_ms,
                treatment_percent,
                treatment_percent_config_count,
                audit_sample_count,
                assignment_integrity_percent,
            )| {
                let (ci_lower, ci_upper) = crate::agent_runtime::wilson_confidence_interval(
                    success_count.max(0) as u64,
                    sample_count.max(0) as u64,
                )?;
                Some(crate::models::ModelExperimentMetricView {
                    task_type: task_type.clone(),
                    experiment_name: experiment_name.clone(),
                    variant,
                    sample_count,
                    success_count,
                    success_rate,
                    success_rate_ci_lower: ci_lower * 100.0,
                    success_rate_ci_upper: ci_upper * 100.0,
                    minimum_sample_size_reached: sample_count
                        >= crate::agent_runtime::MIN_EXPERIMENT_SAMPLE_SIZE,
                    treatment_minus_control_ci_lower: experiment_effects
                        .get(&(task_type.clone(), experiment_name.clone()))
                        .and_then(|effect| effect.0),
                    treatment_minus_control_ci_upper: experiment_effects
                        .get(&(task_type.clone(), experiment_name.clone()))
                        .and_then(|effect| effect.1),
                    comparison_sample_size_reached: experiment_effects
                        .get(&(task_type.clone(), experiment_name.clone()))
                        .is_some_and(|effect| effect.2),
                    treatment_effect_significant: experiment_effects
                        .get(&(task_type.clone(), experiment_name.clone()))
                        .is_some_and(|effect| effect.3),
                    avg_latency_ms,
                    treatment_percent,
                    treatment_percent_config_count,
                    audit_sample_count,
                    assignment_integrity_percent,
                })
            },
        )
        .collect();
    Ok(Json(metrics))
}

pub(crate) async fn list_model_usage_metrics(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<crate::models::ModelUsageMetricView>>, (StatusCode, String)> {
    require_admin(&state, &claims).await?;
    let metrics = sqlx::query_as::<_, crate::models::ModelUsageMetricView>(
        r#"
        SELECT stage AS task_type,
               COUNT(*)::BIGINT AS run_count,
               COALESCE(
                   SUM(COALESCE(NULLIF(usage_json->>'transport_attempt_count', '')::BIGINT, 0)),
                   0
               )::BIGINT AS transport_attempt_count,
               COALESCE(SUM((usage_json->>'request_count')::BIGINT), 0)::BIGINT
                   AS request_count,
               COALESCE(SUM((usage_json->>'prompt_tokens')::BIGINT), 0)::BIGINT
                   AS prompt_tokens,
               COALESCE(SUM((usage_json->>'completion_tokens')::BIGINT), 0)::BIGINT
                   AS completion_tokens,
               COALESCE(SUM((usage_json->>'total_tokens')::BIGINT), 0)::BIGINT
                   AS total_tokens,
               SUM(NULLIF(usage_json->>'estimated_cost_usd', '')::FLOAT8)
                   FILTER (WHERE usage_json ? 'estimated_cost_usd') AS estimated_cost_usd,
               COUNT(*) FILTER (WHERE usage_json ? 'estimated_cost_usd')::BIGINT
                   AS estimated_cost_run_count,
               MAX(finished_at) AS last_updated
        FROM agent_runs
        WHERE finished_at >= NOW() - INTERVAL '30 days'
          AND usage_json IS NOT NULL
          AND (
              COALESCE((usage_json->>'request_count')::BIGINT, 0) > 0
              OR COALESCE(NULLIF(usage_json->>'transport_attempt_count', '')::BIGINT, 0) > 0
          )
        GROUP BY stage
        ORDER BY stage
        "#,
    )
    .fetch_all(&state.db)
    .await
    .map_err(|error| database_failure("读取模型 token 用量", &error))?;
    Ok(Json(metrics))
}

pub(crate) async fn list_business_quality_metrics(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<crate::models::AdminBusinessQualityMetricView>>, (StatusCode, String)> {
    require_admin(&state, &claims).await?;
    let metrics = sqlx::query_as::<_, crate::models::AdminBusinessQualityMetricView>(
        r#"
        WITH project_runs AS (
            SELECT run.project_id,
                   run.status,
                   run.outcome_assessment,
                   run.executed_at
            FROM decision_runs run
            WHERE run.executed_at >= NOW() - INTERVAL '30 days'
        )
        SELECT project.id AS project_id,
               project.name AS project_name,
               project.owner_username,
               COUNT(runs.project_id)::BIGINT AS total_runs,
               COUNT(runs.project_id) FILTER (WHERE runs.status = 'succeeded')::BIGINT
                   AS succeeded_runs,
               COUNT(runs.project_id) FILTER (
                   WHERE runs.status = 'succeeded'
                     AND runs.outcome_assessment IN ('confirmed', 'mismatch')
               )::BIGINT AS conclusive_outcome_runs,
               COUNT(runs.project_id) FILTER (
                   WHERE runs.status = 'succeeded'
                     AND runs.outcome_assessment = 'mismatch'
               )::BIGINT AS mismatch_runs,
               COALESCE(
                   COUNT(runs.project_id) FILTER (
                       WHERE runs.status = 'succeeded'
                         AND runs.outcome_assessment IS NOT NULL
                   )::FLOAT8 * 100
                   / NULLIF(COUNT(runs.project_id) FILTER (WHERE runs.status = 'succeeded'), 0),
                   0
               ) AS feedback_coverage_percent,
               COALESCE(
                   COUNT(runs.project_id) FILTER (
                       WHERE runs.status = 'succeeded'
                         AND runs.outcome_assessment = 'mismatch'
                   )::FLOAT8 * 100
                   / NULLIF(COUNT(runs.project_id) FILTER (
                       WHERE runs.status = 'succeeded'
                         AND runs.outcome_assessment IN ('confirmed', 'mismatch')
                   ), 0),
                   0
               ) AS mismatch_rate_percent,
               MAX(runs.executed_at) AS last_run_at
        FROM decision_projects project
        JOIN project_runs runs ON runs.project_id = project.id
        GROUP BY project.id, project.name, project.owner_username
        ORDER BY last_run_at DESC, project.id
        "#,
    )
    .fetch_all(&state.db)
    .await
    .map_err(|error| database_failure("读取业务效果指标", &error))?;
    Ok(Json(metrics))
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
        .map_err(|error| sqlx::Error::Protocol(format!("admin 密码加密失败: {error}")))?;
    sqlx::query(
        "INSERT INTO users (username, password_hash, role) VALUES ('admin', $1, 'admin')
         ON CONFLICT (username) DO UPDATE SET
             role = 'admin',
             password_hash = CASE
                 WHEN users.password_hash LIKE $2 THEN EXCLUDED.password_hash
                 ELSE users.password_hash
             END",
    )
    .bind(hash)
    .bind("$2%")
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
    fn experiment_effect_counts_are_scoped_by_task_and_name() {
        let totals = experiment_totals_by_group([
            ("requirement_analysis", "trial", "treatment", 30, 24),
            ("requirement_analysis", "trial", "control", 30, 15),
            ("decision_planning", "trial", "treatment", 30, 9),
            ("decision_planning", "trial", "control", 30, 15),
        ]);

        assert_eq!(totals.len(), 2);
        assert_eq!(
            totals.get(&("requirement_analysis".into(), "trial".into())),
            Some(&(Some((24, 30)), Some((15, 30))))
        );
        assert_eq!(
            totals.get(&("decision_planning".into(), "trial".into())),
            Some(&(Some((9, 30)), Some((15, 30))))
        );
    }

    #[test]
    fn bearer_scheme_is_case_insensitive_and_rejects_empty_tokens() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("Authorization", "bearer token-value".parse().unwrap());
        assert_eq!(bearer_token(&headers), Some("token-value"));

        headers.insert("Authorization", "Bearer   ".parse().unwrap());
        assert_eq!(bearer_token(&headers), None);
    }

    #[test]
    fn client_key_uses_forwarded_ip_only_when_trusted() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("X-Real-IP", "203.0.113.10".parse().unwrap());
        let peer_addr = Some("127.0.0.1:3000".parse().unwrap());

        assert_eq!(client_key(&headers, peer_addr, true), "203.0.113.10");
        assert_eq!(client_key(&headers, peer_addr, false), "127.0.0.1");
    }

    fn test_work_nonce() -> String {
        let challenge = [0u8; 16];
        (0u64..)
            .find(|nonce| captcha_work_is_valid(&challenge, 8, &nonce.to_string()))
            .expect("test work nonce should exist")
            .to_string()
    }

    #[test]
    fn captcha_is_single_use_and_case_insensitive() {
        let work_nonce = test_work_nonce();
        let store = std::sync::Mutex::new(std::collections::HashMap::from([(
            "challenge".to_string(),
            CaptchaChallenge {
                answer_digest: captcha_answer_digest("test-secret", "challenge", "Ab3K9"),
                work_challenge: [0u8; 16],
                work_difficulty: 8,
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
            &work_nonce,
            "test-client"
        ));
        assert!(!consume_captcha(
            &store,
            "test-secret",
            "challenge",
            "AB3K9",
            &work_nonce,
            "test-client"
        ));
    }

    #[test]
    fn invalid_work_nonce_does_not_consume_captcha_attempt() {
        let work_nonce = test_work_nonce();
        let store = std::sync::Mutex::new(std::collections::HashMap::from([(
            "challenge".to_string(),
            CaptchaChallenge {
                answer_digest: captcha_answer_digest("test-secret", "challenge", "ABC12"),
                work_challenge: [0u8; 16],
                work_difficulty: 8,
                created_at: std::time::Instant::now(),
                ttl_seconds: 120,
                attempts: 0,
                client_binding: captcha_client_binding("test-secret", "test-client"),
            },
        )]));

        assert!(!consume_captcha(
            &store,
            "test-secret",
            "challenge",
            "WRONG",
            "not-a-number",
            "test-client"
        ));
        assert_eq!(store.lock().unwrap()["challenge"].attempts, 0);

        assert!(consume_captcha(
            &store,
            "test-secret",
            "challenge",
            "ABC12",
            &work_nonce,
            "test-client"
        ));
    }

    #[test]
    fn expired_captcha_is_rejected() {
        let store = std::sync::Mutex::new(std::collections::HashMap::from([(
            "expired".to_string(),
            CaptchaChallenge {
                answer_digest: captcha_answer_digest("test-secret", "expired", "AB123"),
                work_challenge: [0u8; 16],
                work_difficulty: 8,
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
            "0",
            "test-client"
        ));
    }

    #[test]
    fn captcha_bound_to_client() {
        let work_nonce = test_work_nonce();
        let store = std::sync::Mutex::new(std::collections::HashMap::from([(
            "challenge".to_string(),
            CaptchaChallenge {
                answer_digest: captcha_answer_digest("test-secret", "challenge", "ABC12"),
                work_challenge: [0u8; 16],
                work_difficulty: 8,
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
            &work_nonce,
            "client-2"
        ));
        for _ in 0..CAPTCHA_MAX_ATTEMPTS {
            assert!(!consume_captcha(
                &store,
                "test-secret",
                "challenge",
                "WRONG",
                &work_nonce,
                "client-2"
            ));
        }
        // 正确的客户端应该成功
        assert!(consume_captcha(
            &store,
            "test-secret",
            "challenge",
            "ABC12",
            &work_nonce,
            "client-1"
        ));
    }

    #[test]
    fn generated_captcha_expression_has_a_valid_answer() {
        for _ in 0..100 {
            let (expression, answer) = generate_captcha_expression();
            let characters: Vec<_> = expression.chars().collect();
            assert_eq!(characters.len(), 5);
            assert!(characters.iter().all(|character| character.is_ascii_digit()
                || matches!(character, '+' | '-' | 'X' | '=' | '?')));
            assert_eq!(characters[3], '=');
            assert_eq!(characters[4], '?');

            let left = characters[0].to_digit(10).unwrap();
            let right = characters[2].to_digit(10).unwrap();
            let expected = match characters[1] {
                '+' => left + right,
                '-' => left - right,
                'X' => left * right,
                _ => panic!("unexpected captcha operator"),
            };
            assert_eq!(answer, expected.to_string());
        }
    }

    #[test]
    fn captcha_answer_validation_accepts_numeric_input() {
        assert!(captcha_answer_is_well_formed("0"));
        assert!(captcha_answer_is_well_formed(" 18 "));
        assert!(captcha_answer_is_well_formed("081"));
        assert!(!captcha_answer_is_well_formed(""));
        assert!(!captcha_answer_is_well_formed("1a"));
        assert!(!captcha_answer_is_well_formed("1234"));
    }

    #[test]
    fn captcha_pow_difficulty_adapts_to_login_pressure() {
        assert_eq!(captcha_pow_difficulty(14, 0), 14);
        assert_eq!(captcha_pow_difficulty(14, 4), 15);
        assert_eq!(captcha_pow_difficulty(14, 8), 16);
        assert_eq!(captcha_pow_difficulty(20, 10), 20);
        assert_eq!(captcha_pow_difficulty(8, 10), 10);
    }

    #[test]
    fn captcha_svg_contains_noise_and_all_glyphs() {
        let svg = captcha_svg("8+3=?");
        assert!(svg.starts_with("<svg "));
        assert!(svg.contains("<circle "));
        assert!(svg.contains("<path "));
        assert!(svg.matches("<path ").count() >= 9);
        assert!(svg.contains("<rect "));
        assert!(svg.contains("skewX("));
        assert!(!svg.contains("8+3=?"));
    }
}
