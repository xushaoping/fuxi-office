use leptos::prelude::*;

use crate::models::{DecisionProject, DecisionVersion};
use crate::state::Theme;

/// 全局应用上下文，在整个 DmnApp 中共享
#[derive(Clone, Copy)]
pub struct AppContext {
    pub token: Signal<Option<String>>,
    pub user_role: ReadSignal<String>,
    pub theme: ReadSignal<Theme>,
    pub current_project: ReadSignal<Option<DecisionProject>>,
    pub current_version: ReadSignal<Option<DecisionVersion>>,
}

/// 提供应用上下文到子组件树
pub fn provide_app_context(
    token: Signal<Option<String>>,
    user_role: ReadSignal<String>,
    theme: ReadSignal<Theme>,
    current_project: ReadSignal<Option<DecisionProject>>,
    current_version: ReadSignal<Option<DecisionVersion>>,
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
