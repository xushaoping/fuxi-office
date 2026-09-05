use axum::{
    Router,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
};

use crate::auth::{
    auth_middleware, create_user, delete_user, ensure_admin_user, get_me, list_users, login,
    update_user,
};
use crate::types::AppState;
use sqlx::postgres::PgPool;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::services::ServeDir;

mod api;
mod auth;
mod decision_analytics;
mod dmn;
mod execution_trace;
mod feel;
mod knowledge;
mod models;
mod nl_dsl;
mod project_access;
mod projects;
mod types;
mod version_review;
mod version_verification;
mod workflow;
mod workflow_api;

// CSR 页面外壳：cargo-leptos 的 build 会清空 site 目录且不生成 index.html（CSR 模式），
// 因此页面模板由服务端直接返回，不依赖 target/site/index.html 文件。
//
// 模板文件见项目根 index.html（单一来源）：服务端通过 include_str! 内嵌返回；
// script/build-release.sh 会把它复制为 target/site/index.html，使静态部署同样可用。
//
// 注意：cargo-leptos 会把 wasm-bindgen 生成的 {name}_bg.wasm 重命名为 {name}.wasm
// （为兼容 leptos HydrationScripts 的约定），但生成的 JS 默认仍请求 *_bg.wasm。
// 因此这里必须显式传入 wasm 路径，否则 init() 会 404。
const INDEX_HTML: &str = include_str!("../index.html");

// 根路径返回 CSR 页面外壳
async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

enum ApiError {
    Database,
}

impl From<sqlx::Error> for ApiError {
    fn from(_: sqlx::Error) -> Self {
        ApiError::Database
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            ApiError::Database => (StatusCode::INTERNAL_SERVER_ERROR, "服务器内部错误"),
        };
        (status, message).into_response()
    }
}

#[tokio::main]
async fn main() {
    // 1. 读取 .env 中的 DATABASE_URL 并连接 PostgreSQL
    dotenvy::dotenv().ok();
    let database_url = match std::env::var("DATABASE_URL") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            eprintln!("环境变量 DATABASE_URL 未设置（请在 .env 中配置）");
            return;
        }
    };
    let pool = match PgPool::connect(&database_url).await {
        Ok(pool) => pool,
        Err(error) => {
            eprintln!("连接 PostgreSQL 失败，请确认数据库已启动且 script/init.sql 已执行：{error}");
            return;
        }
    };
    if let Err(error) = ensure_admin_user(&pool).await {
        eprintln!("初始化 admin 用户失败：{error}");
        return;
    }

    // 2. 定义 API 路由
    //    认证接口（登录/当前用户）AppState（数据库连接 + JWT 密钥）
    let jwt_secret = match std::env::var("JWT_SECRET") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            eprintln!("环境变量 JWT_SECRET 未设置（请在 .env 中配置）");
            return;
        }
    };
    let state = AppState {
        db: pool,
        jwt_secret,
    };
    let llm_state = Arc::new(api::LlmState::from_env());

    // 1. 公开路由（不需要鉴权）：仅登录；用户由超级管理员创建
    let public_routes = Router::new().route("/login", post(login));

    // 2. 受保护路由：先经 auth_middleware 校验 JWT，用户名注入请求扩展供 handler 提取
    //    注意：axum 0.8 路径参数语法为 {id}（0.7 的 :id 会被当作字面量导致 404）
    let protected_routes = Router::new()
        .route("/me", get(get_me))
        .merge(knowledge::routes())
        .merge(projects::routes())
        .merge(workflow_api::routes(llm_state.clone()))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let admin_routes = Router::new()
        .route("/admin/users", get(list_users).post(create_user))
        .route(
            "/admin/users/{username}",
            axum::routing::put(update_user).delete(delete_user),
        )
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    // 3. 组合 API 路由：merge 要求两侧 Router 的 state 类型一致（均为 AppState）
    let api_routes = Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .merge(admin_routes)
        .merge(api::public_routes())
        .merge(projects::public_service_routes())
        // DMN generation invokes the configured LLM and must not be reachable
        .merge(
            api::dms_routes(llm_state).route_layer(axum::middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        );

    // 4. 全局路由：
    //    - nest 接受 Router<S> 并与之共享 state 类型（nest_service 只接受已实现
    //      Service 的 Router<()>，此处 Router<AppState> 不能直接用 nest_service）
    //    - 根路径由服务端返回页面外壳；fallback_service 负责其余静态资源（pkg、style 等）
    let app = Router::new()
        .nest("/api", api_routes)
        .route("/", get(index))
        // The CSR router owns this path after the initial page load. Serving the
        // same shell here keeps a direct visit or browser refresh from returning 404.
        .route("/login", get(index))
        .route("/admin", get(index))
        .nest_service("/pkg", ServeDir::new("target/site/pkg"))
        .nest_service("/style", ServeDir::new("target/site/style"))
        .fallback_service(ServeDir::new("target/site"))
        .with_state(state);

    let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".to_string());
    let addr: SocketAddr = match bind_addr.parse() {
        Ok(addr) => addr,
        Err(error) => {
            eprintln!("BIND_ADDR 无效（{bind_addr}）：{error}");
            return;
        }
    };
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!(
                "无法监听 {addr}：{error}。如端口被占用，请设置 BIND_ADDR，例如 127.0.0.1:3001"
            );
            return;
        }
    };
    println!("🚀 CSR 服务启动于 http://{addr}");
    axum::serve(listener, app).await.unwrap();
}
