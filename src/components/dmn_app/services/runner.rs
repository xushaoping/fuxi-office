use leptos::prelude::*;

use crate::components::common::{SelectOption, WorkbenchSelect};
use crate::models::PublishedDecisionService;

#[component]
pub fn ServiceInvocationForm(
    published_services: ReadSignal<Vec<PublishedDecisionService>>,
    selected_service_id: ReadSignal<Option<String>>,
    service_business_reference: ReadSignal<String>,
    set_service_business_reference: WriteSignal<String>,
    service_input: ReadSignal<String>,
    set_service_input: WriteSignal<String>,
    service_busy: ReadSignal<bool>,
    on_select_service: Callback<String>,
    on_execute: Callback<web_sys::MouseEvent>,
) -> impl IntoView {
    view! {
        <div class="service-runner-controls">
            <WorkbenchSelect
                label="决策服务"
                options=Signal::derive(move || published_services.get().into_iter().map(|service| SelectOption::new(service.id, service.name)).collect())
                value=Signal::derive(move || selected_service_id.get().unwrap_or_default())
                disabled=Signal::derive(move || service_busy.get() || published_services.get().is_empty())
                placeholder=Signal::derive(|| "请选择决策服务".to_string())
                on_change=on_select_service
            />
            <label>
                <span>"业务关联号"</span>
                <input
                    type="text"
                    maxlength="255"
                    placeholder="订单号、申请号或案件号"
                    prop:value=service_business_reference
                    on:input=move |event| set_service_business_reference.set(event_target_value(&event))
                    disabled=move || service_busy.get()
                />
            </label>
            <label class="service-input-editor">
                <span>"调用输入 JSON"</span>
                <textarea
                    rows="5"
                    spellcheck="false"
                    prop:value=service_input
                    on:input=move |event| set_service_input.set(event_target_value(&event))
                ></textarea>
            </label>
            <button
                class="button button-primary"
                on:click=move |event| on_execute.run(event)
                disabled=move || service_busy.get() || selected_service_id.get().is_none() || service_business_reference.get().trim().is_empty()
            >{move || if service_busy.get() { "调用中..." } else { "调用服务" }}</button>
        </div>
    }
}
