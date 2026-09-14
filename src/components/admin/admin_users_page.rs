use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;

use crate::api;
use crate::components::auth::auth_token;
use crate::models::{AdminUser, CreateUser, UpdateUser};
use crate::state::AppState;

#[component]
pub fn AdminUsersPage() -> impl IntoView {
    let token = auth_token();
    let navigate = use_navigate();
    let app_state = expect_context::<AppState>();
    let users = RwSignal::new(Vec::<AdminUser>::new());
    let username = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let confirm_password = RwSignal::new(String::new());
    let role = RwSignal::new("user".to_string());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let status = RwSignal::new(None::<String>);

    let reload = move || {
        let Some(token_value) = token.get() else {
            return;
        };
        spawn_local(async move {
            match api::list_admin_users(&token_value).await {
                Ok(items) => users.set(items),
                Err(message) => error.set(Some(message)),
            }
        });
    };
    Effect::new(move |_| {
        if token.get().is_some() {
            reload();
        }
    });

    let create = move |_| {
        let Some(token_value) = token.get() else {
            return;
        };
        let name = username.get().trim().to_string();
        let secret = password.get();
        let confirmation = confirm_password.get();
        if name.is_empty() || secret.len() < 6 || busy.get() {
            return;
        }
        if secret != confirmation {
            error.set(Some("两次输入的密码不一致".into()));
            return;
        }
        busy.set(true);
        error.set(None);
        let selected_role = role.get();
        spawn_local(async move {
            match api::create_admin_user(
                &token_value,
                CreateUser {
                    username: name,
                    password: secret,
                    role: selected_role,
                },
            )
            .await
            {
                Ok(user) => {
                    users.update(|items| items.push(user));
                    username.set(String::new());
                    password.set(String::new());
                    confirm_password.set(String::new());
                    status.set(Some("用户已添加".into()));
                }
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    let update_role = move |user: AdminUser, new_role: String| {
        let Some(token_value) = token.get() else {
            return;
        };
        if busy.get() || user.username == "admin" && new_role != "admin" {
            return;
        }
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match api::update_admin_user(
                &token_value,
                &user.username,
                UpdateUser {
                    role: new_role,
                    password: None,
                },
            )
            .await
            {
                Ok(updated) => users.update(|items| {
                    if let Some(item) = items
                        .iter_mut()
                        .find(|item| item.username == updated.username)
                    {
                        *item = updated;
                    }
                }),
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    let delete_user = move |username: String| {
        let Some(token_value) = token.get() else {
            return;
        };
        if busy.get() || username == "admin" {
            return;
        }
        if !web_sys::window()
            .and_then(|win| {
                win.confirm_with_message(&format!(
                    "确定要删除用户 \"{}\" 吗？此操作无法撤销。",
                    username
                ))
                .ok()
            })
            .unwrap_or(false)
        {
            return;
        }
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match api::delete_admin_user(&token_value, &username).await {
                Ok(()) => {
                    users.update(|items| items.retain(|item| item.username != username));
                    status.set(Some("用户已删除".into()));
                }
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    view! {
        <main class="admin-page admin-users-page">
            <header class="admin-topbar">
                <div class="admin-brand">
                    <div class="brand-mark">
                        <img src="/images/fuxi.png" />
                    </div>
                    <div>
                        <span class="eyebrow">"DMN DECISION WORKSPACE"</span>
                        <strong>"伏羲智能决策建模工作台"</strong>
                    </div>
                </div>
                <button class="button button-quiet" on:click=move |_| {
                    if !app_state.ui.workspace_dirty.get_untracked()
                        || web_sys::window().is_some_and(|window| {
                            window.confirm_with_message("当前工作台有未确认的需求或模型变更，本地草稿会保留，确定返回吗？").unwrap_or(false)
                        })
                    {
                        navigate("/", Default::default());
                    }
                }>
                    <span aria-hidden="true">"←"</span> "返回工作台"
                </button>
            </header>
            <div class="admin-content">
                <section class="admin-heading">
                    <div>
                        <span class="yaml-kicker">"USER DIRECTORY / ACCESS CONTROL"</span>
                        <h1>"系统用户管理"</h1>
                        <p>"集中管理工作台账号，并为每位成员分配系统级访问角色。"</p>
                    </div>
                    <div class="admin-summary">
                        <span class="admin-summary-label">"当前账号"</span>
                        <strong>"admin"</strong>
                        <span class="admin-role-badge admin-role-badge-primary">"超级管理员"</span>
                    </div>
                </section>
                <div class="admin-layout">
                    <section class="management-form-panel">
                        <div class="admin-section-heading">
                            <div><span class="yaml-kicker">"ADD ACCOUNT"</span><h2>"添加用户"</h2></div>
                            <span class="admin-section-index">"01"</span>
                        </div>
                        <p class="admin-section-note">"创建后，用户可使用账号登录并参与已分配的项目。"</p>
                        <div class="admin-form-grid management-form-grid">
                            <label><span>"用户名"</span><input maxlength="255" placeholder="输入用户名" prop:value=username on:input=move |event| username.set(event_target_value(&event)) /></label>
                            <label><span>"初始密码"</span><input type="password" placeholder="至少 6 位" prop:value=password on:input=move |event| password.set(event_target_value(&event)) /></label>
                            <label><span>"确认密码"</span><input type="password" placeholder="再次输入密码" prop:value=confirm_password on:input=move |event| confirm_password.set(event_target_value(&event)) /></label>
                            <label><span>"系统角色"</span><select prop:value=role on:change=move |event| role.set(event_target_value(&event))><option value="user">"普通用户"</option><option value="admin">"超级管理员"</option></select></label>
                        </div>
                        <Show when=move || !confirm_password.get().is_empty() && password.get() != confirm_password.get()>
                            <div class="error-msg" role="alert">"两次输入的密码不一致"</div>
                        </Show>
                        <button class="button button-primary admin-submit" on:click=create disabled=move || busy.get() || username.get().trim().is_empty() || password.get().len() < 6 || password.get() != confirm_password.get()>
                            <span aria-hidden="true">"+"</span> {move || if busy.get() { "处理中..." } else { "创建用户" }}
                        </button>
                    </section>
                    <section class="management-list-panel">
                        <div class="admin-section-heading">
                            <div><span class="yaml-kicker">"ACTIVE ACCOUNTS"</span><h2>"用户列表"</h2></div>
                            <span class="admin-user-count">{move || users.get().len()} " 个用户"</span>
                        </div>
                        <div class="admin-list-header"><span>"用户"</span><span>"系统角色"</span><span>"操作"</span></div>
                        <div class="admin-user-list">
                            <For each=move || users.get() key=|user| user.username.clone() let:user>
                                {
                                    let role_value = user.role.clone();
                                    let role_label = if role_value == "admin" { "超级管理员" } else { "普通用户" };
                                    let role_class = if role_value == "admin" { "admin-role-badge admin-role-badge-primary" } else { "admin-role-badge" };
                                    let username_for_delete = user.username.clone();
                                    let username_for_select = user.username.clone();
                                    let username_for_disabled = user.username.clone();
                                    let username_for_title = user.username.clone();
                                    view! {
                                        <article class="admin-user-row">
                                            <div class="admin-user-identity">
                                                <span class="admin-avatar">{user.username.chars().next().unwrap_or('?').to_uppercase().to_string()}</span>
                                                <div><strong>{user.username.clone()}</strong><small>{format!("加入于 {}", user.created_at.format("%Y-%m-%d"))}</small></div>
                                            </div>
                                            <div class="admin-user-role">
                                                <span class=role_class>{role_label}</span>
                                                <select aria-label="修改系统角色" prop:value=role_value on:change={let user = user.clone(); move |event| update_role(user.clone(), event_target_value(&event))} disabled=move || username_for_select == "admin" || busy.get()>
                                                    <option value="user">"普通用户"</option>
                                                    <option value="admin">"超级管理员"</option>
                                                </select>
                                            </div>
                                            <div class="admin-user-actions">
                                                <button
                                                    class="button button-danger button-small"
                                                    on:click=move |_| delete_user(username_for_delete.clone())
                                                    disabled=move || username_for_disabled == "admin" || busy.get()
                                                    title=move || if username_for_title == "admin" { "不能删除 admin 用户" } else { "删除用户" }
                                                >
                                                    "删除"
                                                </button>
                                            </div>
                                        </article>
                                    }
                                }
                            </For>
                            <Show when=move || users.get().is_empty()><div class="admin-empty">"暂无用户记录"</div></Show>
                        </div>
                    </section>
                </div>
                <Show when=move || status.get().is_some()><div class="member-status">{move || status.get().unwrap_or_default()}</div></Show>
                <Show when=move || error.get().is_some()><div class="error-msg">{move || error.get().unwrap_or_default()}</div></Show>
            </div>
        </main>
    }
}
