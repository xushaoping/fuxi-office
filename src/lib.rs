// 说明：本库的目标是 WASM 前端（csr feature）与共享数据结构。
// dmn/feel 由 pub mod nl_dsl（agent 路径）在非 wasm 环境下引用；
// 其余后端实现（决策分析、执行轨迹、版本验收等）由 src/main.rs 的
// 自包含模块树（api/projects 等）直接编译使用，不属于本库消费者，
// 故不在本库声明，以免产生大量无调用方（dead code）警告。
#[cfg(not(target_family = "wasm"))]
// 引擎主体专供二进制目标（src/main.rs 自包含模块树）使用，库内仅
// nl_dsl::agent 引用 parse/compile 入口，其余导出与实现视为库内未用
#[allow(dead_code, unused_imports)]
mod dmn;
#[cfg(not(target_family = "wasm"))]
// 同 dmn：库内引用来自 dmn 模块内部（FEEL 解析），其余引擎能力供二进制使用
#[allow(dead_code, unused_imports)]
mod feel;
#[cfg(not(target_family = "wasm"))]
pub mod nl_dsl;
#[cfg(not(target_family = "wasm"))]
pub mod requirement_complexity;

// 共享的数据结构（前后端通用）
pub mod models;
pub mod workflow;

// ===== 以下为 CSR 前端代码：仅在启用 `csr` feature（编译 WASM）时构建 =====
#[cfg(all(feature = "csr", target_family = "wasm"))]
pub mod api;
#[cfg(all(feature = "csr", target_family = "wasm"))]
mod components;
#[cfg(all(feature = "csr", target_family = "wasm"))]
pub mod state;
#[cfg(all(feature = "csr", target_family = "wasm"))]
pub mod utils;

// Wasm 入口函数，cargo-leptos 默认会寻找名为 hydrate 的函数挂载 WASM
#[cfg(all(feature = "csr", target_family = "wasm"))]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(components::App);
}
