use anyhow::{Context, Result, bail};
use axum::{
    Extension, Router,
    response::Html,
    routing::{get, post},
};

use crate::auth::{
    auth_middleware, captcha, create_user, delete_user, ensure_admin_user, get_me,
    list_business_quality_metrics, list_decision_explanation_admin_metrics,
    list_model_experiment_metrics, list_model_performance_metrics, list_model_usage_metrics,
    list_users, login, update_user,
};
use crate::types::AppState;
use sqlx::postgres::PgPoolOptions;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;
use tower_http::services::ServeDir;

mod agent_runtime;
mod api;
mod auth;
mod decision_analytics;
mod dmn;
mod execution_trace;
mod feel;
mod health;
mod knowledge;
mod models;
mod nl_dsl;
mod project_access;
mod projects;
mod prompt_registry;
mod rag;
mod requirement_complexity;
mod storage;
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
async fn index() -> Html<String> {
    Html(INDEX_HTML.replace(
        "__FUXI_ASSET_VERSION__",
        option_env!("FUXI_ASSET_VERSION").unwrap_or(env!("CARGO_PKG_VERSION")),
    ))
}

// 后台清理任务：定期清理过期的验证码和频率限制窗口
async fn cleanup_expired_entries(state: AppState) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
    loop {
        interval.tick().await;

        // 清理过期验证码
        if let Ok(mut captchas) = state.captchas.lock() {
            let before = captchas.len();
            captchas.retain(|_, challenge| !challenge.is_expired());
            let removed = before.saturating_sub(captchas.len());
            if removed > 0 {
                println!("[维护] 清理了 {} 个过期验证码", removed);
            }
        }

        // 清理过期的频率限制窗口（60秒窗口）
        let cleanup_rate_limits =
            |limits: &Arc<std::sync::Mutex<HashMap<String, types::RateLimitWindow>>>| {
                if let Ok(mut limits) = limits.lock() {
                    let before = limits.len();
                    limits.retain(|_, window| window.created_at.elapsed().as_secs() <= 60);
                    before.saturating_sub(limits.len())
                } else {
                    0
                }
            };

        let captcha_removed = cleanup_rate_limits(&state.captcha_rate_limits);
        let login_removed = cleanup_rate_limits(&state.login_rate_limits);
        let verify_removed = cleanup_rate_limits(&state.captcha_verify_limits);

        if captcha_removed + login_removed + verify_removed > 0 {
            println!(
                "[维护] 清理了频率限制窗口 - 验证码请求: {}, 登录: {}, 验证码验证: {}",
                captcha_removed, login_removed, verify_removed
            );
        }
    }
}

struct StartupConfig {
    database_url: String,
    jwt_secret: String,
    bind_addr: SocketAddr,
    shutdown_grace: Duration,
    shutdown_drain: Duration,
}

fn validate_credentials(jwt_secret: &str, admin_password: &str, production: bool) -> Result<()> {
    if jwt_secret.trim().is_empty() {
        bail!("JWT_SECRET 必须设置");
    }
    if admin_password.trim().is_empty()
        || admin_password == "change_this_admin_password"
        || !(6..=1024).contains(&admin_password.len())
    {
        bail!("ADMIN_PASSWORD 必须设置，长度为 6-1024 字节且不能使用默认值");
    }
    if production {
        if jwt_secret.len() < 32
            || jwt_secret.starts_with("replace-with-")
            || jwt_secret.starts_with("change_this_")
        {
            bail!("生产 JWT_SECRET 必须使用至少 32 字节的独立随机密钥");
        }
        if admin_password.starts_with("replace-with-") {
            bail!("生产 ADMIN_PASSWORD 不能使用示例占位值");
        }
    }
    Ok(())
}

fn seconds_setting(name: &str, default: u64, maximum: u64) -> Result<Duration> {
    let seconds = match std::env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .with_context(|| format!("{name} 必须为整数秒数"))?,
        Err(std::env::VarError::NotPresent) => default,
        Err(_) => bail!("{name} 必须为有效文本"),
    };
    if seconds > maximum {
        bail!("{name} 不能超过 {maximum} 秒");
    }
    Ok(Duration::from_secs(seconds))
}

impl StartupConfig {
    fn from_env() -> Result<Self> {
        let required = |name| -> Result<String> {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
                .with_context(|| format!("环境变量 {name} 必须设置"))
        };
        let database_url = required("DATABASE_URL")?;
        let jwt_secret = required("JWT_SECRET")?;
        let admin_password = required("ADMIN_PASSWORD")?;
        let environment = std::env::var("APP_ENV")
            .or_else(|_| std::env::var("RUST_ENV"))
            .or_else(|_| std::env::var("LEPTOS_ENV"))
            .unwrap_or_default()
            .to_ascii_lowercase();
        validate_credentials(
            &jwt_secret,
            &admin_password,
            matches!(environment.as_str(), "production" | "prod"),
        )?;
        let bind_addr = std::env::var("BIND_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:3000".to_string())
            .parse()
            .context("BIND_ADDR 必须为有效监听地址")?;
        let shutdown_grace = seconds_setting("SHUTDOWN_GRACE_SECONDS", 30, 300)?;
        let shutdown_drain = seconds_setting("SHUTDOWN_DRAIN_SECONDS", 2, 30)?;
        if shutdown_grace.is_zero() || shutdown_drain >= shutdown_grace {
            bail!("SHUTDOWN_GRACE_SECONDS 必须大于 0 且大于 SHUTDOWN_DRAIN_SECONDS");
        }
        Ok(Self {
            database_url,
            jwt_secret,
            bind_addr,
            shutdown_grace,
            shutdown_drain,
        })
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    match run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("服务启动或运行失败：{error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<()> {
    dotenvy::dotenv().ok();
    // Validate all mandatory configuration and the complete schema before any
    // backfill, database mutation or external Embedding/LLM initialization.
    let config = StartupConfig::from_env()?;
    let mut shutdown_signal = health::ShutdownSignal::register()?;
    let signal = shutdown_signal.wait();
    tokio::pin!(signal);
    let initialized = tokio::select! {
        result = initialize(&config) => result?,
        result = &mut signal => {
            result?;
            println!("启动阶段收到停止信号，已取消初始化");
            return Ok(());
        }
    };
    serve_initialized(initialized, &config, signal).await
}

async fn serve_initialized(
    initialized: InitializedServer,
    config: &StartupConfig,
    signal: impl std::future::Future<Output = Result<()>>,
) -> Result<()> {
    let InitializedServer {
        pool,
        readiness,
        listener,
        app,
        mut workers,
    } = initialized;
    let addr = config.bind_addr;
    println!("🚀 CSR 服务启动于 http://{addr}");
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = shutdown_rx.await;
    });
    let mut server = tokio::spawn(std::future::IntoFuture::into_future(server));
    let signal_result = tokio::select! {
        result = &mut server => {
            readiness.drain();
            workers.abort_all();
            return result.context("HTTP 服务任务失败")?.context("HTTP 服务失败");
        }
        result = signal => result,
    };
    // Mark unready first, allowing the proxy to stop routing before closing the
    // listener. Background loops are cancelled; durable work recovers on restart.
    readiness.drain();
    workers.abort_all();
    let deadline = tokio::time::Instant::now() + config.shutdown_grace;
    println!("收到停止信号，开始排空服务");
    tokio::time::sleep(config.shutdown_drain).await;
    let _ = shutdown_tx.send(());
    let result = match tokio::time::timeout_at(deadline, &mut server).await {
        Ok(result) => result
            .context("HTTP 服务任务失败")?
            .context("HTTP 服务失败"),
        Err(_) => {
            server.abort();
            Err(anyhow::anyhow!("优雅停止超时，已终止 HTTP 服务"))
        }
    };
    let _ = tokio::time::timeout_at(deadline, async {
        while workers.join_next().await.is_some() {}
    })
    .await;
    let pool_closed = tokio::time::timeout_at(deadline, pool.close())
        .await
        .is_ok();
    signal_result?;
    result?;
    if !pool_closed {
        bail!("数据库连接排空超时");
    }
    Ok(())
}

struct InitializedServer {
    pool: sqlx::PgPool,
    readiness: health::Readiness,
    listener: tokio::net::TcpListener,
    app: Router,
    workers: JoinSet<()>,
}

async fn initialize(config: &StartupConfig) -> Result<InitializedServer> {
    let pool = PgPoolOptions::new()
        .acquire_timeout(Duration::from_secs(5))
        .connect(&config.database_url)
        .await
        .context("连接 PostgreSQL 失败")?;
    health::check_deployment_baseline(&pool)
        .await
        .context("数据库发布基线未通过；请先以应用角色完成迁移与权限检查")?;
    let readiness = health::Readiness::new(pool.clone());
    let addr = config.bind_addr;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("无法绑定 BIND_ADDR；请检查地址和端口占用")?;
    match knowledge::backfill_knowledge_chunks(&pool).await {
        Ok(count) if count > 0 => println!("已为 {count} 个知识源生成检索分块"),
        Ok(_) => {}
        Err(error) => return Err(error.into()),
    }
    storage::partitions::maintain_decision_run_partitions(&pool)
        .await
        .context("初始化决策运行分区失败")?;
    storage::materialized_views::refresh_daily_run_stats(&pool)
        .await
        .context("初始化决策运行每日统计失败")?;
    ensure_admin_user(&pool)
        .await
        .context("初始化 admin 用户失败")?;
    let orchestration_stale_after = workflow_api::orchestration_stale_timeout_from_env();
    match workflow_api::recover_stale_orchestrations(&pool, orchestration_stale_after).await {
        Ok(count) if count > 0 => println!("已回收 {count} 个失联的 Agent 编排"),
        Ok(_) => {}
        Err(error) => eprintln!("回收失联 Agent 编排失败：{error}"),
    }
    if !prompt_registry::schema_ready(&pool)
        .await
        .context("检查提示词注册表结构失败")?
    {
        bail!("提示词注册表迁移未完成；请先执行升级与数据库基线检查");
    }
    match prompt_registry::recover_stale_evaluation_jobs(
        &pool,
        prompt_registry::EVALUATION_JOB_STALE_AFTER,
    )
    .await
    {
        Ok(count) if count > 0 => println!("已回收 {count} 个失联的提示词评测作业"),
        Ok(_) => {}
        Err(error) => eprintln!("回收失联提示词评测作业失败：{error}"),
    }

    // 2. 定义 API 路由
    //    认证接口（登录/当前用户）AppState（数据库连接 + JWT 密钥）
    let captcha_pow_difficulty = std::env::var("CAPTCHA_POW_DIFFICULTY")
        .ok()
        .and_then(|value| value.trim().parse::<u8>().ok())
        .map(|value| value.clamp(8, 20))
        .unwrap_or(14);
    let state = AppState {
        db: pool.clone(),
        jwt_secret: config.jwt_secret.clone(),
        captcha_pow_difficulty,
        captchas: Arc::new(std::sync::Mutex::new(HashMap::new())),
        captcha_rate_limits: Arc::new(std::sync::Mutex::new(HashMap::new())),
        login_rate_limits: Arc::new(std::sync::Mutex::new(HashMap::new())),
        captcha_verify_limits: Arc::new(std::sync::Mutex::new(HashMap::new())),
        trust_proxy_headers: std::env::var("TRUST_PROXY_HEADERS")
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false),
    };
    let llm_state = Arc::new(api::LlmState::from_env(&pool).await);

    // Keep worker handles so shutdown cannot leave maintenance loops running.
    let mut workers = JoinSet::new();
    workers.spawn(cleanup_expired_entries(state.clone()));
    let orchestration_recovery_pool = pool.clone();
    workers.spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            match workflow_api::recover_stale_orchestrations(
                &orchestration_recovery_pool,
                orchestration_stale_after,
            )
            .await
            {
                Ok(count) if count > 0 => println!("已回收 {count} 个失联的 Agent 编排"),
                Ok(_) => {}
                Err(error) => eprintln!("回收失联 Agent 编排失败：{error}"),
            }
        }
    });
    let evaluation_recovery_pool = pool.clone();
    workers.spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            match prompt_registry::recover_stale_evaluation_jobs(
                &evaluation_recovery_pool,
                prompt_registry::EVALUATION_JOB_STALE_AFTER,
            )
            .await
            {
                Ok(count) if count > 0 => println!("已回收 {count} 个失联的提示词评测作业"),
                Ok(_) => {}
                Err(error) => eprintln!("回收失联提示词评测作业失败：{error}"),
            }
        }
    });

    workers.spawn(knowledge::backfill_knowledge_chunks_daily(pool.clone()));
    workers.spawn(storage::partitions::maintain_decision_run_partitions_daily(
        pool.clone(),
    ));
    workers.spawn(storage::materialized_views::refresh_daily_run_stats_daily(
        pool.clone(),
    ));
    workers.spawn(
        storage::optimization_effectiveness::track_proposal_effectiveness_daily(pool.clone()),
    );
    workers.spawn(projects::scan_decision_drift_events_daily(state.clone()));
    workers.spawn(projects::deliver_drift_webhooks(state.clone()));

    // 1. 公开路由（不需要鉴权）：仅登录；用户由超级管理员创建
    let public_routes = Router::new()
        .route("/login", post(login))
        .route("/captcha", get(captcha))
        .with_state(state.clone());

    // 2. 受保护路由：先经 auth_middleware 校验 JWT，用户名注入请求扩展供 handler 提取
    //    注意：axum 0.8 路径参数语法为 {id}（0.7 的 :id 会被当作字面量导致 404）
    let protected_routes = Router::new()
        .route("/me", get(get_me))
        .merge(knowledge::routes())
        .merge(projects::routes(llm_state.clone()))
        .merge(workflow_api::routes(llm_state.clone()))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let admin_routes = Router::new()
        .merge(prompt_registry::routes(llm_state.clone()))
        .route("/admin/users", get(list_users).post(create_user))
        .route(
            "/admin/model-performance",
            get(list_model_performance_metrics),
        )
        .route(
            "/admin/decision-explanation-metrics",
            get(list_decision_explanation_admin_metrics),
        )
        .route(
            "/admin/model-experiments",
            get(list_model_experiment_metrics),
        )
        .route("/admin/model-usage", get(list_model_usage_metrics))
        .route(
            "/admin/business-quality",
            get(list_business_quality_metrics),
        )
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
        .route("/ready", get(health::ready))
        .layer(Extension(readiness.clone()))
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
        .route("/admin/users", get(index))
        .route("/admin/intelligence", get(index))
        .route("/admin/prompts", get(index))
        .route("/projects", get(index))
        .nest_service("/pkg", ServeDir::new("target/site/pkg"))
        .nest_service("/style", ServeDir::new("target/site/style"))
        .fallback_service(ServeDir::new("target/site"))
        .with_state(state);

    Ok(InitializedServer {
        pool,
        readiness,
        listener,
        app,
        workers,
    })
}

#[cfg(test)]
mod startup_tests {
    use super::*;

    #[test]
    fn production_credentials_reject_short_secrets_and_example_values() {
        assert!(validate_credentials("short", "strong-admin-password", true).is_err());
        assert!(
            validate_credentials(
                "replace-with-a-random-secret-at-least-32-characters",
                "strong-admin-password",
                true,
            )
            .is_err()
        );
        assert!(
            validate_credentials(&"a".repeat(32), "replace-with-a-strong-password", true).is_err()
        );
        assert!(validate_credentials(&"a".repeat(32), "strong-admin-password", true).is_ok());
        assert!(validate_credentials("dev-key", "dev-password", false).is_ok());
    }

    #[test]
    fn invalid_admin_configuration_fails_before_database_initialization() {
        for password in ["", "     ", "short", "change_this_admin_password"] {
            assert!(validate_credentials("dev-key", password, false).is_err());
        }
        assert!(validate_credentials("", "dev-password", false).is_err());
    }

    // These are HTTP fixtures, without business initialization or database I/O.
    // Run with the isolated deployment tests where loopback listeners are allowed.
    async fn shutdown_fixture(
        grace: Duration,
    ) -> (
        SocketAddr,
        Arc<tokio::sync::Notify>,
        Arc<tokio::sync::Notify>,
        tokio::sync::oneshot::Sender<()>,
        tokio::task::JoinHandle<Result<()>>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let pool =
            sqlx::PgPool::connect_lazy("postgresql://unused:unused@127.0.0.1:1/unused").unwrap();
        let readiness = health::Readiness::new(pool.clone());
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let handler_entered = entered.clone();
        let handler_release = release.clone();
        let app = Router::new()
            .route(
                "/slow",
                get(move || {
                    let entered = handler_entered.clone();
                    let release = handler_release.clone();
                    async move {
                        entered.notify_one();
                        release.notified().await;
                        "completed"
                    }
                }),
            )
            .nest("/api", api::public_routes());
        let initialized = InitializedServer {
            pool,
            readiness,
            listener,
            app,
            workers: JoinSet::new(),
        };
        let config = StartupConfig {
            database_url: String::new(),
            jwt_secret: String::new(),
            bind_addr: addr,
            shutdown_grace: grace,
            shutdown_drain: Duration::ZERO,
        };
        let (shutdown, rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            serve_initialized(initialized, &config, async {
                rx.await.context("fixture shutdown sender dropped")
            })
            .await
        });
        (addr, entered, release, shutdown, server)
    }

    #[tokio::test]
    #[ignore = "requires loopback HTTP fixture; no business service or database"]
    async fn graceful_shutdown_completes_an_inflight_request() {
        let (addr, entered, release, shutdown, server) =
            shutdown_fixture(Duration::from_secs(2)).await;
        let request = tokio::spawn(async move {
            reqwest::get(format!("http://{addr}/slow"))
                .await
                .unwrap()
                .text()
                .await
                .unwrap()
        });
        tokio::time::timeout(Duration::from_secs(2), entered.notified())
            .await
            .unwrap();
        shutdown.send(()).unwrap();
        tokio::task::yield_now().await;
        release.notify_one();
        assert_eq!(request.await.unwrap(), "completed");
        assert!(
            tokio::time::timeout(Duration::from_secs(2), server)
                .await
                .unwrap()
                .unwrap()
                .is_ok()
        );
    }

    #[tokio::test]
    #[ignore = "requires loopback HTTP fixture; no business service or database"]
    async fn graceful_shutdown_bounds_an_unfinished_request() {
        let (addr, entered, release, shutdown, server) =
            shutdown_fixture(Duration::from_millis(100)).await;
        let request =
            tokio::spawn(async move { reqwest::get(format!("http://{addr}/slow")).await });
        tokio::time::timeout(Duration::from_secs(2), entered.notified())
            .await
            .unwrap();
        shutdown.send(()).unwrap();
        let error = tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(error.to_string().contains("优雅停止超时"));
        release.notify_one();
        request.abort();
    }
}
