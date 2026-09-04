#[cfg(not(target_family = "wasm"))]
mod decision_analytics;
#[cfg(not(target_family = "wasm"))]
mod dmn;
#[cfg(not(target_family = "wasm"))]
mod execution_trace;
#[cfg(not(target_family = "wasm"))]
mod feel;
#[cfg(not(target_family = "wasm"))]
pub mod nl_dsl;

// 共享的数据结构（前后端通用）
pub mod models;
#[cfg(not(target_family = "wasm"))]
mod version_review;
#[cfg(not(target_family = "wasm"))]
mod version_verification;
pub mod workflow;

// ===== 以下为 CSR 前端代码：仅在启用 `csr` feature（编译 WASM）时构建 =====
#[cfg(all(feature = "csr", target_family = "wasm"))]
pub mod api;
#[cfg(all(feature = "csr", target_family = "wasm"))]
mod components;

// Wasm 入口函数，cargo-leptos 默认会寻找名为 hydrate 的函数挂载 WASM
#[cfg(all(feature = "csr", target_family = "wasm"))]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(components::App);
}
