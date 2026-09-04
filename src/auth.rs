use axum::{
    Extension, Json,
    extract::{Path, State},
    http::{Request, StatusCode},
};
use chrono::{Duration, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};

// 引入加密和数据库模块
use crate::models::{AdminUser, CreateUser, UpdateUser, UserRecord};
use crate::types::{AppState, AuthPayload, Claims, LoginResponse};
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use axum::middleware::Next;
use axum::response::Response;

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

// 登录接口
pub(crate) async fn login(
    State(state): State<AppState>,
    Json(payload): Json<AuthPayload>,
) -> Result<Json<LoginResponse>, StatusCode> {
    // 1. 从数据库查询用户
    let username = payload.username.trim();
    let user = sqlx::query_as::<_, UserRecord>(
        "SELECT username, password_hash, role FROM users WHERE username = $1",
    )
    .bind(username)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::UNAUTHORIZED)?; // 用户不存在

    // 2. 校验密码
    verify_password(&user.password_hash, &payload.password)
        .map_err(|_| StatusCode::UNAUTHORIZED)?; // 密码错误

    // 3. 签发 JWT
    let exp = (Utc::now() + Duration::hours(24)).timestamp() as usize;
    let claims = Claims {
        sub: user.username,
        exp,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.jwt_secret.as_bytes()),
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(LoginResponse { token }))
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
}
