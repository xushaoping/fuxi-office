use leptos::prelude::*;

use crate::components::workspace::{CopyStatus, public_service_curl};
use crate::models::{DecisionServiceApiKey, PublishedDecisionService};

/// 展示凭证和一次性密钥；请求、复制及密钥生命周期仍由页面管理。
#[component]
pub fn ServiceCredentialsPanel(
    published_services: ReadSignal<Vec<PublishedDecisionService>>,
    selected_service_id: ReadSignal<Option<String>>,
    service_keys: ReadSignal<Vec<DecisionServiceApiKey>>,
    service_key_name: ReadSignal<String>,
    set_service_key_name: WriteSignal<String>,
    service_key_secret: ReadSignal<Option<String>>,
    service_key_service_id: ReadSignal<Option<String>>,
    service_key_copy_status: ReadSignal<CopyStatus>,
    service_curl_copy_status: ReadSignal<CopyStatus>,
    service_key_busy: ReadSignal<bool>,
    service_key_error: ReadSignal<Option<String>>,
    on_create_service_key: Callback<leptos::ev::MouseEvent>,
    on_copy_service_key: Callback<leptos::ev::MouseEvent>,
    on_copy_service_curl: Callback<leptos::ev::MouseEvent>,
    on_revoke_service_key: Callback<i64>,
) -> impl IntoView {
    view! {
        <div class="service-public-access">
            <div class="service-public-access-heading">
                <div>
                    <strong>"对外调用凭证"</strong>
                    <small>"无需登录，凭证仅绑定当前决策服务；发布新版本后仍按线上版本执行"</small>
                </div>
                <span>"X-Decision-Key"</span>
            </div>
            <div class="service-key-create">
                <input
                    type="text"
                    maxlength="120"
                    placeholder="凭证名称，例如 order-service"
                    prop:value=service_key_name
                    on:input=move |event| set_service_key_name.set(event_target_value(&event))
                    disabled=move || service_key_busy.get() || selected_service_id.get().is_none()
                />
                <button
                    class="button button-primary"
                    on:click=move |event| on_create_service_key.run(event)
                    disabled=move || service_key_busy.get() || selected_service_id.get().is_none() || service_key_name.get().trim().is_empty()
                >{move || if service_key_busy.get() { "处理中..." } else { "生成调用凭证" }}</button>
            </div>
            <Show when=move || service_key_secret.get().is_some()>
                <div class="service-key-secret" role="alert">
                    <div class="service-key-secret-heading">
                        <strong>"完整密钥，仅显示一次"</strong>
                        <button
                            type="button"
                            class="button button-quiet"
                            title="复制完整密钥"
                            aria-label="复制完整密钥"
                            on:click=move |event| on_copy_service_key.run(event)
                        >
                            {move || match service_key_copy_status.get() {
                                CopyStatus::Idle => "复制完整密钥",
                                CopyStatus::Copied => "已复制",
                                CopyStatus::Failed => "复制失败",
                            }}
                        </button>
                    </div>
                    <input
                        class="service-key-secret-value"
                        type="text"
                        readonly=true
                        spellcheck="false"
                        aria-label="完整决策服务调用密钥"
                        prop:value=move || service_key_secret.get().unwrap_or_default()
                    />
                    <div class="service-key-curl-heading">
                        <strong>"可直接执行的 curl"</strong>
                        <button
                            type="button"
                            class="button button-quiet"
                            title="复制可执行 curl"
                            aria-label="复制可执行 curl"
                            on:click=move |event| on_copy_service_curl.run(event)
                        >
                            {move || match service_curl_copy_status.get() {
                                CopyStatus::Idle => "复制 curl",
                                CopyStatus::Copied => "已复制",
                                CopyStatus::Failed => "复制失败",
                            }}
                        </button>
                    </div>
                    <pre class="service-key-curl"><code>{move || {
                        let secret = service_key_secret.get().unwrap_or_default();
                        let service_id = service_key_service_id.get();
                        published_services.get().into_iter()
                            .find(|service| Some(service.id.clone()) == service_id)
                            .map(|service| public_service_curl(&service, &secret))
                            .unwrap_or_default()
                    }}</code></pre>
                </div>
            </Show>
            <Show when=move || service_key_error.get().is_some()>
                <div class="service-key-error">{move || service_key_error.get().unwrap_or_default()}</div>
            </Show>
            <Show when=move || !service_keys.get().is_empty()>
                <div class="service-key-list">
                    <For each=move || service_keys.get() key=|key| key.id let:key>
                        <div class=format!("service-key-row{}", if key.revoked_at.is_some() { " service-key-revoked" } else { "" })>
                            <div>
                                <strong>{key.name.clone()}</strong>
                                <small>{format!("凭证 ID：{} · 服务：{} · 仅用于识别，不能直接调用", key.key_prefix, key.service_id)}</small>
                            </div>
                            <span>{move || if key.revoked_at.is_some() { "已撤销" } else { "有效" }}</span>
                            <button
                                type="button"
                                class="button button-quiet danger-action"
                                title="删除并立即停用凭证"
                                aria-label="删除并立即停用凭证"
                                on:click=move |_| on_revoke_service_key.run(key.id)
                                disabled=move || service_key_busy.get() || key.revoked_at.is_some()
                            >"删除"</button>
                        </div>
                    </For>
                </div>
            </Show>
        </div>
    }
}
