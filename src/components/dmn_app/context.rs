use leptos::prelude::*;

use super::types::Theme;
use crate::models::{DecisionProject, DecisionVersion};

/// 全局应用上下文，在整个 DmnApp 中共享
#[derive(Clone, Copy)]
pub struct AppContext {
    pub token: Signal<Option<String>>,
    pub user_role: RwSignal<String>,
    pub theme: RwSignal<Theme>,
    pub current_project: RwSignal<Option<DecisionProject>>,
    pub current_version: RwSignal<Option<DecisionVersion>>,
}

/// 提供应用上下文到子组件树
pub fn provide_app_context(
    token: Signal<Option<String>>,
    user_role: RwSignal<String>,
    theme: RwSignal<Theme>,
    current_project: RwSignal<Option<DecisionProject>>,
    current_version: RwSignal<Option<DecisionVersion>>,
) -> AppContext {
    let ctx = AppContext {
        token,
        user_role,
        theme,
        current_project,
        current_version,
    };
    provide_context(ctx);
    ctx
}

/// 从上下文中获取应用上下文
pub fn use_app_context() -> AppContext {
    expect_context::<AppContext>()
}
