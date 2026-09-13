use leptos::prelude::*;

use crate::components::workspace::public_service_endpoint;
use crate::models::PublishedDecisionService;

/// 仅展示接口与字段；复制行为由页面回调负责。
#[component]
pub fn ServiceContractList(
    published_services: ReadSignal<Vec<PublishedDecisionService>>,
    service_endpoint_copied: ReadSignal<Option<String>>,
    on_copy_endpoint: Callback<(String, String)>,
) -> impl IntoView {
    view! {
        <div class="service-contract-list">
            <For each=move || published_services.get() key=|service| service.id.clone() let:service>
                {let endpoint = public_service_endpoint(&service);
                let endpoint_service_id = service.id.clone();
                let endpoint_for_copy = endpoint.clone();
                let service_id_for_copy = endpoint_service_id.clone();
                let service_id_for_label = endpoint_service_id.clone();
                view! { <article class="service-contract">
                    <div><strong>{service.name.clone()}</strong><span>{format!("v{}", service.version_number)}</span></div>
                    <div class="service-endpoint">
                        <code>{format!("POST {endpoint}")}</code>
                        <button
                            type="button"
                            class="button button-quiet"
                            title="复制公开调用 URL"
                            aria-label="复制公开调用 URL"
                            on:click=move |_| on_copy_endpoint.run((service_id_for_copy.clone(), endpoint_for_copy.clone()))
                        >{move || if service_endpoint_copied.get().as_deref() == Some(service_id_for_label.as_str()) { "已复制" } else { "复制 URL" }}</button>
                    </div>
                    <div class="service-fields">
                        <div>
                            <small>"输入"</small>
                            <For each=move || service.inputs.clone() key=|field| format!("{}:{}", field.source, field.name) let:field>
                                <span><strong>{field.name}</strong><em>{field.type_ref}</em></span>
                            </For>
                        </div>
                        <div>
                            <small>"输出"</small>
                            <For each=move || service.outputs.clone() key=|field| field.name.clone() let:field>
                                <span><strong>{field.name}</strong><em>{field.type_ref}</em></span>
                            </For>
                        </div>
                    </div>
                </article> } }
            </For>
        </div>
    }
}
