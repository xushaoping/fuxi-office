use gloo_storage::{LocalStorage, Storage};
use leptos::leptos_dom::helpers::window_event_listener;
use leptos::prelude::*;
use leptos_router::components::A;

use crate::components::dmn_app::context::use_app_context;
use crate::components::workspace::RunStatus;
use crate::state::Theme;

#[component]
pub fn Header(
    status: ReadSignal<RunStatus>,
    user_id: ReadSignal<String>,
    user_role: ReadSignal<String>,
    validation_running: ReadSignal<bool>,
    xml: ReadSignal<Option<String>>,
    has_unsaved_changes: Signal<bool>,
    on_validate: Callback<()>,
) -> impl IntoView {
    let ctx = use_app_context();
    let app_state = expect_context::<crate::state::AppState>();

    let logout = move |_| {
        if app_state.ui.workspace_dirty.get_untracked()
            && !web_sys::window().is_some_and(|window| {
                window
                    .confirm_with_message(
                        "当前工作台有未确认的需求或模型变更，本地草稿会保留，确定退出吗？",
                    )
                    .unwrap_or(false)
            })
        {
            return;
        }
        let _ = LocalStorage::delete("auth_token");
        let _ = web_sys::window().and_then(|window| window.location().set_href("/login").ok());
    };

    let beforeunload_listener = window_event_listener(leptos::ev::beforeunload, move |event| {
        if has_unsaved_changes.get_untracked() {
            event.set_return_value("");
        }
    });
    on_cleanup(move || beforeunload_listener.remove());

    let confirm_navigation = move |event: leptos::ev::MouseEvent| {
        if has_unsaved_changes.get_untracked() {
            let confirmed = web_sys::window().is_some_and(|window| {
                window
                    .confirm_with_message(
                        "当前工作台有未确认的需求或模型变更，本地草稿会保留，确定离开吗？",
                    )
                    .unwrap_or(false)
            });
            if !confirmed {
                event.prevent_default();
            }
        }
    };

    view! {
        <header class="topbar">
            <div class="brand-lockup">
                <div class="brand-mark">
                    <img src="/images/fuxi.png" alt="伏羲" />
                </div>
                <div>
                    <p class="eyebrow">"DMN DECISION WORKSPACE"</p>
                    <h1>"伏羲智能决策建模工作台"</h1>
                </div>
            </div>
            <div class="topbar-actions">
                <div class="run-status-wrap">
                    <span class="toolbar-label">"当前状态"</span>
                    <div class=move || format!("run-status {}", status.get().class())>
                        <span class="status-dot"></span>
                        {move || status.get().label()}
                    </div>
                </div>
                <button
                    class="theme-toggle"
                    title=move || ctx.theme.get().toggle_label()
                    aria-label=move || ctx.theme.get().toggle_label()
                    on:click=move |_| app_state.set_theme(ctx.theme.get_untracked().toggle())
                >
                    <span class="theme-toggle-icon" aria-hidden="true">
                        {move || if ctx.theme.get() == Theme::Dark { "☼" } else { "☾" }}
                    </span>
                    <span>{move || if ctx.theme.get() == Theme::Dark { "浅色" } else { "深色" }}</span>
                </button>
                <button
                    class="button"
                    on:click=move |_| on_validate.run(())
                    disabled=move || xml.get().is_none() || validation_running.get()
                >
                    <svg class="icon" viewBox="0 0 1024 1024" version="1.1" xmlns="http://www.w3.org/2000/svg" p-id="23661" width="200" height="200"><path d="M266.666667 128A96 96 0 0 0 170.666667 224v216.661333a316.586667 316.586667 0 0 1 64-23.68V224a32 32 0 0 1 32-32h405.333333a32 32 0 0 1 32 32V832h-90.282667a317.738667 317.738667 0 0 1-30.933333 64h217.216A138.666667 138.666667 0 0 0 938.666667 757.333333V597.333333h-170.666667V224A96 96 0 0 0 672 128h-405.333333z m533.333333 704H768v-170.666667h106.666667v96a74.666667 74.666667 0 0 1-74.666667 74.666667z" fill="#1296db" p-id="23662"></path><path d="M566.741333 533.333333h19.925334a32 32 0 0 0 0-64h-84.906667c24.576 18.133333 46.506667 39.68 64.981333 64zM320 330.666667A32 32 0 0 1 352 298.666667h234.666667a32 32 0 0 1 0 64h-234.666667a32 32 0 0 1-32-32zM235.392 793.856v-7.637333h154.709333v7.68c0 25.6-21.461333 38.826667-77.354666 38.826666-55.850667 0-77.354667-13.226667-77.354667-38.826666zM281.813333 631.509333a30.933333 30.933333 0 1 1 61.866667 0 30.933333 30.933333 0 0 1-61.866667 0z" fill="#1296db" p-id="23663"></path><path d="M312.789333 963.84a247.210667 247.210667 0 1 1 0-494.421333 247.210667 247.210667 0 0 1 0 494.421333z m123.733334-185.344c0-21.333333-17.322667-38.698667-38.656-38.698667H227.669333c-21.333333 0-38.656 17.322667-38.656 38.698667v15.36c0 57.173333 45.525333 85.333333 123.733334 85.333333 78.250667 0 123.776-28.16 123.776-85.248v-15.445333zM390.101333 631.466667a77.354667 77.354667 0 1 0-154.709333 0 77.354667 77.354667 0 0 0 154.709333 0z" fill="#1296db" p-id="23664"></path>
                    </svg>
                    {move || if validation_running.get() { "核验中..." } else { "人工核验" }}
                </button>
                <details class="user-menu">
                    <summary
                        class="user-menu-trigger"
                        aria-label="打开用户菜单"
                        aria-controls="user-menu-popover"
                    >
                        <span class="user-avatar" aria-hidden="true">
                            <span class="user-avatar-initial">
                                {move || user_id.get().chars().next().unwrap_or('用').to_uppercase().to_string()}
                            </span>
                        </span>
                        <span class="user-menu-name">
                            {move || if user_id.get().is_empty() { "当前用户".to_string() } else { user_id.get() }}
                        </span>
                        <span class="user-menu-chevron" aria-hidden="true"></span>
                    </summary>
                    <div id="user-menu-popover" class="user-menu-popover">
                        <div class="user-menu-heading">
                            <div class="user-menu-heading-info">
                                <strong>{move || if user_id.get().is_empty() { "当前用户".to_string() } else { user_id.get() }}</strong>
                                <small>{move || {
                                    let role = user_role.get();
                                    match role.as_str() {
                                        "admin" => "管理员".to_string(),
                                        "user" => "普通用户".to_string(),
                                        "" => "工作台成员".to_string(),
                                        role => role.to_string(),
                                    }
                                }}</small>
                            </div>
                             <svg  viewBox="0 0 1024 1024" version="1.1" xmlns="http://www.w3.org/2000/svg" p-id="2581" width="200" height="200">
                                <path d="M592.2 725.4l-81.7 233.9-79.6-233.9V544.8h161.3z" fill="#FEE1B9" p-id="2582"></path>
                                <path d="M510.5 959.3l81.7-233.9V544.8h-81.7z" fill="#FFD9AC" p-id="2583"></path>
                                <path d="M294.6 384.8c-2.6-26.9 12-50.9 32.8-53.9 20.8-2.9 39.8 16.7 42.7 43.6 2.6 26.9-12 50.9-32.8 53.8-21.1 3-40.1-16.6-42.7-43.5z" fill="#FEE1B9" p-id="2584"></path>
                                <path d="M685.8 428.4c-20.8-2.3-34.8-26-31.3-53 3.5-26.9 23.1-47.1 43.9-44.8 20.8 2.3 34.8 26 31.3 53-3.5 27.2-23.1 47.1-43.9 44.8z" fill="#FFD9AC" p-id="2585"></path>
                                <path d="M512 158s-171.5 7.3-171.5 98.9V443c0 36 58.2 138.7 110.6 143.1 26.3 2 60.6 5 60.6 5s34.5-2.9 60.6-5c52.4-4.4 110-107.1 110-143.1V261.8C682.9 170.5 512 158 512 158z" fill="#FEE1B9" p-id="2586"></path>
                                <path d="M512 158s-171.5 7.3-171.5 98.9V443c0 36 58.2 138.7 110.6 143.1 26.3 2 60.6 5 60.6 5s34.5-2.9 60.6-5c52.4-4.4 110-107.1 110-143.1V261.8C682.9 170.5 512 158 512 158zM512 959.3V671.6s-61.5-15.2-74.9-65c-6.4-24-6.4-18.7-6.4 5.9 0 74-174.1 42.1-221.2 99.2-37.2 51.5-62.9 247.6-62.9 247.6H512z" fill="#FEE1B9" p-id="2587"></path>
                                <path d="M511.7 959.3V656.4s58.5-1.2 69.7-29.6c4.1-10.2 10.8-38.3 10.8-13.8 0 74 174.7 41.6 221.8 98.6 37.2 51.5 62.9 247.6 62.9 247.6H511.7z" fill="#FFD9AC" p-id="2588"></path><path d="M409.6 646.4S240.4 668.6 203 715.5c-30.1 38-50.9 200.5-60.9 244.7H512V673.9l-102.4-27.5zM614.7 646.4s170 22.8 207.5 69.7c30.1 38 50 199.9 59.7 244.1H512V673.9l102.7-27.5z" fill="#D0D4D7" p-id="2589"></path>
                                <path d="M511.7 865l0.3-201.6L407.5 642 512 960.1" fill="#D0D4D7" p-id="2590"></path><path d="M524 723.1l23.7-23.7-35.7-35.7-35.4 35.7 23.7 23.7-32.2 174.1 43.9 62.9 44.2-62.9z" fill="#436B85" p-id="2591"></path><path d="M547.7 699.4L512 663.7l-0.3 0.6v295l0.3 0.8 44.2-62.9L524 723.1z" fill="#315D76" p-id="2592"></path><path d="M512 663.4l84-42.5 43.3 25.5-70.5 100.7z" fill="#FFFFFF" p-id="2593"></path>
                                <path d="M512.3 663.4l-84-42.5-43.3 25.5 85.2 100.7z" fill="#FFFFFF" p-id="2594"></path>
                                <path d="M324.1 367.2h16.4l22-152.2s-49.8-19.6-60 40.4c-10.3 59.7 21.6 111.8 21.6 111.8z m367 0h-16.4L652.8 215s49.8-19.6 60 40.4c10.2 59.7-21.7 111.8-21.7 111.8z" fill="#61402F" p-id="2595"></path>
                                <path d="M343.4 230.2s69.7 54.1 166.2 54.1c96.6 0 187.9-29.9 229.4-111.2-2.3 0.6-74-12.3-163.9-80.8-112.9-86.2-325 41.9-231.7 137.9z" fill="#61402F" p-id="2596"></path>
                            </svg>
                        </div>
                        <div class="user-menu-divider"></div>
                        <A href="/projects" class:user-menu-item=true on:click=confirm_navigation>
                            <svg class="icon" viewBox="0 0 1024 1024" version="1.1" xmlns="http://www.w3.org/2000/svg" p-id="40772" width="200" height="200"><path d="M491.008 966.016L106.88 703.104c-18.944-12.928-21.632-39.808-5.76-56.32 12.416-12.8 32.128-15.104 47.104-5.248L491.52 867.968c12.416 8.192 28.544 8.192 40.832 0l342.4-227.2c14.976-9.856 34.816-7.68 47.232 5.248l0.896 0.896c15.744 16.512 13.056 43.264-5.888 56.192L532.864 966.016c-12.672 8.704-29.184 8.704-41.856 0z" fill="#BAE7FF" p-id="40773"></path><path d="M491.008 799.744L106.88 536.704c-18.944-12.928-21.632-39.808-5.76-56.32 12.416-12.8 32.128-15.104 47.104-5.248L491.52 701.568c12.416 8.192 28.544 8.192 40.832 0l342.4-227.2c14.976-9.984 34.816-7.68 47.232 5.248l0.896 0.896c15.744 16.512 13.056 43.264-5.888 56.192L532.864 799.744c-12.672 8.576-29.184 8.576-41.856 0z" fill="#99DBF2" p-id="40774"></path><path d="M491.008 633.344L115.456 376.192c-21.504-14.72-21.504-46.464 0-61.184L491.008 57.984c12.672-8.576 29.184-8.576 41.856 0l375.552 257.152c21.504 14.72 21.504 46.464 0 61.184L532.864 633.344c-12.672 8.576-29.184 8.576-41.856 0z" fill="#319EEC" p-id="40775"></path><path d="M636.288 279.424L401.024 439.168c-15.104 10.24-35.968 6.272-46.208-8.832s-6.272-35.968 8.832-46.208l235.264-159.744c15.104-10.24 35.968-6.272 46.208 8.832s6.272 35.968-8.832 46.208z" fill="#FFFFFF" p-id="40776"></path></svg>
                            "项目管理"
                        </A>
                        <Show when=move || user_role.get() == "admin">
                            <A href="/admin" class:user-menu-item=true on:click=confirm_navigation>
                                <svg class="icon" viewBox="0 0 1024 1024" version="1.1" xmlns="http://www.w3.org/2000/svg" p-id="33735" width="200" height="200"><path d="M946.176 770.001455l0.232727-86.574546s-57.157818-11.357091-58.181818-15.173818a262.656 262.656 0 0 0-3.397818-12.427636c6.702545-12.381091 38.912-39.703273 38.912-39.703273l-64.512-85.736727s-41.751273 34.397091-52.130909 39.284363a189.160727 189.160727 0 0 0-15.266909-5.166545c-5.585455-1.396364-21.131636-56.459636-21.131637-56.459637l-86.760727-0.744727s-9.402182 53.294545-23.365818 56.925091a93.230545 93.230545 0 0 0-14.149818 5.352727c-10.379636-5.073455-52.130909-40.029091-52.130909-40.029091l-64.465455 87.272728s32.302545 32.535273 34.443636 43.426909a221.090909 221.090909 0 0 0-3.863272 10.426182c-13.870545 4.887273-53.341091 12.706909-53.341091 12.706909l0.232727 86.574545s45.800727 9.541818 54.551273 17.175273c1.815273 4.933818 3.863273 9.821091 6.144 14.615273-10.565818 13.218909-38.865455 35.234909-38.865455 35.234909l66.420364 86.574545s52.224-33.838545 60.834909-35.746909c4.235636 1.815273 8.564364 3.584 13.032727 5.12 7.819636 2.792727 13.405091 53.154909 13.405091 53.154909l86.714182 0.325818s2.978909-49.477818 9.309091-51.432727c6.283636-2.001455 12.474182-4.282182 18.432-6.888727 7.307636 2.885818 41.472 36.957091 41.472 36.957091l85.643636-87.272728s-31.837091-21.969455-40.541091-33.792a210.850909 210.850909 0 0 0 6.516364-15.080727c2.885818-7.866182 55.808-18.897455 55.808-18.897454z m-220.439273 65.62909a109.381818 109.381818 0 1 1 0-218.763636 109.381818 109.381818 0 0 1 0 218.763636zM447.674182 503.156364c99.421091 0 200.610909-97.745455 200.610909-201.262546C648.238545 198.376727 551.889455 93.090909 452.468364 93.090909c-99.421091 0-198.283636 107.845818-198.283637 211.316364 0 103.517091 94.021818 198.749091 193.442909 198.749091z m43.752727 43.101091H292.584727c-49.803636 0-124.322909 25.925818-149.131636 103.470545-24.808727 77.591273-24.808727 77.591273-49.757091 155.275636-12.474182 38.958545 169.658182 58.786909 350.766545 58.181819 60.136727-0.186182 19.176727-49.570909 19.176728-112.174546 0-142.661818 121.669818-204.753455 27.787636-204.753454z" fill="#1296db" p-id="33736"></path></svg>
                                "用户管理"
                            </A>
                        </Show>
                        <button class="user-menu-item user-menu-danger" on:click=logout>
                            <svg class="icon" viewBox="0 0 1024 1024" version="1.1" xmlns="http://www.w3.org/2000/svg" p-id="41789" width="200" height="200"><path d="M512 1024a512 512 0 1 1 512-512 512 512 0 0 1-512 512z m38.826667-784.469333a38.826667 38.826667 0 0 0-77.653334 0v192a36.394667 36.394667 0 0 0 38.826667 38.4 38.4 38.4 0 0 0 38.826667-38.4v-192z m194.133333 72.917333a37.845333 37.845333 0 0 0-54.186667-3.84 36.608 36.608 0 0 0-3.84 53.717333 222.848 222.848 0 0 1 58.026667 153.6 232.96 232.96 0 0 1-465.92 0 222.848 222.848 0 0 1 58.026667-153.6 36.949333 36.949333 0 0 0-3.84-53.717333 37.845333 37.845333 0 0 0-54.186667 3.84 309.205333 309.205333 0 1 0 544 203.392 301.696 301.696 0 0 0-78.08-203.392z" fill="#FF6E42" p-id="41790"></path></svg>
                            "退出登录"
                        </button>
                    </div>
                </details>
            </div>
        </header>
    }
}
