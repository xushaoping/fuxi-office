use leptos::prelude::*;

#[component]
pub fn AccessibleInput(
    id: String,
    label: String,
    #[prop(optional)] error: Option<String>,
    #[prop(optional)] required: bool,
    value: RwSignal<String>,
) -> impl IntoView {
    let error_id = format!("{id}-error");
    let described_by = error.as_ref().map(|_| error_id.clone());
    view! {
        <div class="form-field">
            <label class="form-label" for=id.clone()>{label}{required.then(|| view! { <span class="required-asterisk" aria-label="必填">"*"</span> })}</label>
            <input id=id type="text" class="form-input" prop:value=move || value.get() on:input=move |ev| value.set(event_target_value(&ev)) aria-required=required aria-invalid=error.is_some() aria-describedby=described_by />
            {error.map(|message| view! { <div id=error_id class="form-error" role="alert">{message}</div> })}
        </div>
    }
}
