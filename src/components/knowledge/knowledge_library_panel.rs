use leptos::prelude::*;
use leptos::task::spawn_local;
use std::collections::HashSet;

use crate::api;
use crate::components::common::{SelectOption, WorkbenchDatePicker, WorkbenchSelect};
use crate::models::{DecisionProject, KnowledgeSource, SaveKnowledgeSource};

fn knowledge_type_label(value: &str) -> &'static str {
    match value {
        "term" => "企业术语",
        "field" => "字段目录",
        "policy" => "制度政策",
        _ => "知识源",
    }
}

fn knowledge_effective_label(source: &KnowledgeSource) -> &'static str {
    if source.status == "archived" {
        "已归档"
    } else if source.is_effective {
        "当前有效"
    } else {
        "非当前有效"
    }
}
#[component]
pub fn KnowledgeLibraryPanel(
    token: RwSignal<Option<String>>,
    current_project: ReadSignal<Option<DecisionProject>>,
) -> impl IntoView {
    let sources = RwSignal::new(Vec::<KnowledgeSource>::new());
    let attached_ids = RwSignal::new(HashSet::<i64>::new());
    let selected_id = RwSignal::new(None::<i64>);
    let source_type = RwSignal::new("term".to_string());
    let name = RwSignal::new(String::new());
    let description = RwSignal::new(String::new());
    let content = RwSignal::new(String::new());
    let version_label = RwSignal::new(String::new());
    let source_uri = RwSignal::new(String::new());
    let valid_from = RwSignal::new(String::new());
    let valid_until = RwSignal::new(String::new());
    let filter = RwSignal::new("all".to_string());
    let query = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);

    let clear_form = move || {
        selected_id.set(None);
        source_type.set("term".into());
        name.set(String::new());
        description.set(String::new());
        content.set(String::new());
        version_label.set(String::new());
        source_uri.set(String::new());
        valid_from.set(String::new());
        valid_until.set(String::new());
    };

    Effect::new(move |_| {
        let Some(token_value) = token.get() else {
            sources.set(Vec::new());
            attached_ids.set(HashSet::new());
            return;
        };
        let project_id = current_project.get().map(|project| project.id);
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match api::list_knowledge_sources(&token_value).await {
                Ok(items) => sources.set(items),
                Err(message) => error.set(Some(message)),
            }
            if let Some(project_id) = project_id {
                match api::list_project_knowledge_sources(&token_value, project_id).await {
                    Ok(items) => attached_ids.set(items.into_iter().map(|item| item.id).collect()),
                    Err(message) => error.set(Some(message)),
                }
            } else {
                attached_ids.set(HashSet::new());
            }
            busy.set(false);
        });
    });

    let edit_source = move |source: KnowledgeSource| {
        selected_id.set(Some(source.id));
        source_type.set(source.source_type);
        name.set(source.name);
        description.set(source.description);
        content.set(source.content);
        version_label.set(source.version_label);
        source_uri.set(source.source_uri);
        valid_from.set(
            source
                .valid_from
                .map(|date| date.to_string())
                .unwrap_or_default(),
        );
        valid_until.set(
            source
                .valid_until
                .map(|date| date.to_string())
                .unwrap_or_default(),
        );
    };

    let save_source = move |_| {
        let Some(token_value) = token.get() else {
            return;
        };
        let parse_date = |value: String| {
            if value.trim().is_empty() {
                Ok(None)
            } else {
                chrono::NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d")
                    .map(Some)
                    .map_err(|_| "有效期必须使用 YYYY-MM-DD 格式".to_string())
            }
        };
        let valid_from_value = match parse_date(valid_from.get()) {
            Ok(value) => value,
            Err(message) => {
                error.set(Some(message));
                return;
            }
        };
        let valid_until_value = match parse_date(valid_until.get()) {
            Ok(value) => value,
            Err(message) => {
                error.set(Some(message));
                return;
            }
        };
        let payload = SaveKnowledgeSource {
            source_type: source_type.get(),
            name: name.get(),
            description: description.get(),
            content: content.get(),
            version_label: version_label.get(),
            source_uri: source_uri.get(),
            valid_from: valid_from_value,
            valid_until: valid_until_value,
            status: "active".into(),
        };
        if payload.name.trim().is_empty() || payload.content.trim().is_empty() {
            error.set(Some("知识名称和内容不能为空".into()));
            return;
        }
        let editing = selected_id.get();
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            let result = match editing {
                Some(id) => api::update_knowledge_source(&token_value, id, &payload).await,
                None => api::create_knowledge_source(&token_value, &payload).await,
            };
            match result {
                Ok(saved) => {
                    sources.update(|items| {
                        if let Some(item) = items.iter_mut().find(|item| item.id == saved.id) {
                            *item = saved.clone();
                        } else {
                            items.insert(0, saved);
                        }
                    });
                    clear_form();
                }
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    let archive_source = move |source_id: i64| {
        let Some(token_value) = token.get() else {
            return;
        };
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match api::archive_knowledge_source(&token_value, source_id).await {
                Ok(()) => {
                    sources.update(|items| {
                        if let Some(item) = items.iter_mut().find(|item| item.id == source_id) {
                            item.status = "archived".into();
                            item.is_effective = false;
                        }
                    });
                    attached_ids.update(|ids| {
                        ids.remove(&source_id);
                    });
                    if selected_id.get() == Some(source_id) {
                        clear_form();
                    }
                }
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    let toggle_attachment = move |source_id: i64, attached: bool| {
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            error.set(Some("请先选择决策项目".into()));
            return;
        };
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match api::set_project_knowledge_source(&token_value, project.id, source_id, attached)
                .await
            {
                Ok(()) => attached_ids.update(|ids| {
                    if attached {
                        ids.insert(source_id);
                    } else {
                        ids.remove(&source_id);
                    }
                }),
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    view! {
        <section class="knowledge-library-panel" aria-label="企业知识库">
            <div class="knowledge-library-heading">
                <div>
                    <span class="yaml-kicker">"GOVERNED KNOWLEDGE"</span>
                    <strong>"企业知识库"</strong>
                    <small>{move || format!("{} 个知识源 · 当前项目已绑定 {} 个", sources.get().len(), attached_ids.get().len())}</small>
                </div>
                <div class="knowledge-toolbar">
                    <input
                        class="resource-search"
                        type="search"
                        placeholder="搜索名称、说明或内容"
                        prop:value=query
                        on:input=move |event| query.set(event_target_value(&event))
                    />
                    <div class="knowledge-filters">
                        <For each=move || vec![("all", "全部"), ("attached", "已绑定"), ("term", "术语"), ("field", "字段"), ("policy", "制度")] key=|item| item.0 let:item>
                            <button class=move || if filter.get() == item.0 { "knowledge-filter is-active" } else { "knowledge-filter" } on:click=move |_| filter.set(item.0.into())>{item.1}</button>
                        </For>
                    </div>
                </div>
            </div>
            <div class="knowledge-library-body">
                <div class="knowledge-source-browser">
                    <div class="resource-column-heading">
                        <strong>"知识目录"</strong>
                        <button class="button button-quiet" on:click=move |_| clear_form()>"新建"</button>
                    </div>
                    <div class="knowledge-source-list">
                    <Show when=move || busy.get() && sources.get().is_empty()>
                        <div class="knowledge-empty">"正在读取知识库..."</div>
                    </Show>
                    <For
                        each=move || {
                            let selected = filter.get();
                            let search = query.get().trim().to_lowercase();
                            sources.get().into_iter().filter(|source| {
                                (selected == "all"
                                    || (selected == "attached" && attached_ids.get().contains(&source.id))
                                    || source.source_type == selected)
                                    && (search.is_empty()
                                        || source.name.to_lowercase().contains(&search)
                                        || source.description.to_lowercase().contains(&search)
                                        || source.content.to_lowercase().contains(&search))
                            }).collect::<Vec<_>>()
                        }
                        key=|source| format!("{}:{}:{}", source.id, source.status, source.updated_at)
                        let:source
                    >
                        {
                            let source_for_edit = source.clone();
                            let source_id = source.id;
                            let archived = source.status == "archived";
                            let validity = match (source.valid_from, source.valid_until) {
                                (Some(from), Some(until)) => format!("{from} 至 {until}"),
                                (Some(from), None) => format!("{from} 起"),
                                (None, Some(until)) => format!("有效至 {until}"),
                                (None, None) => "未设置有效期".into(),
                            };
                            view! {
                                <article class=move || {
                                    let attached = attached_ids.get().contains(&source_id);
                                    let selected = selected_id.get() == Some(source_id);
                                    format!("knowledge-source{}{}{}", if attached { " is-attached" } else { "" }, if selected { " is-selected" } else { "" }, if archived { " is-archived" } else { "" })
                                }>
                                    <div class="knowledge-source-title">
                                        <span>{knowledge_type_label(&source.source_type)}</span>
                                        <strong>{source.name.clone()}</strong>
                                        <small class=if source.is_effective { "is-effective" } else { "" }>{knowledge_effective_label(&source)}</small>
                                    </div>
                                    <p>{source.description.clone()}</p>
                                    <div class="knowledge-source-meta">
                                        <code>{if source.version_label.is_empty() { "未标注版本".into() } else { format!("版本 {}", source.version_label) }}</code>
                                        <span>{validity}</span>
                                    </div>
                                    <div class="knowledge-source-actions">
                                        <label title="绑定后，当前有效内容会进入该项目的智能上下文">
                                            <input type="checkbox" prop:checked=move || attached_ids.get().contains(&source_id) on:change=move |event| toggle_attachment(source_id, event_target_checked(&event)) disabled=archived />
                                            <span>"绑定当前项目"</span>
                                        </label>
                                        <button class="button button-quiet" on:click=move |_| edit_source(source_for_edit.clone())>"编辑"</button>
                                        <button class="button button-quiet" on:click=move |_| archive_source(source_id) disabled=move || busy.get() || archived>"归档"</button>
                                    </div>
                                </article>
                            }
                        }
                    </For>
                    <Show when=move || {
                        let selected = filter.get();
                        let search = query.get().trim().to_lowercase();
                        !busy.get() && !sources.get().iter().any(|source| {
                            (selected == "all"
                                || (selected == "attached" && attached_ids.get().contains(&source.id))
                                || source.source_type == selected)
                                && (search.is_empty()
                                    || source.name.to_lowercase().contains(&search)
                                    || source.description.to_lowercase().contains(&search)
                                    || source.content.to_lowercase().contains(&search))
                        })
                    }>
                        <div class="knowledge-empty">"没有匹配的知识源"</div>
                    </Show>
                    </div>
                </div>
                <div class="knowledge-editor">
                    <div class="knowledge-editor-heading">
                        <div><strong>{move || if selected_id.get().is_some() { "编辑知识源" } else { "新建知识源" }}</strong><small>"保存后可按项目绑定，只有当前有效内容会进入智能上下文"</small></div>
                        <button class="button button-quiet" on:click=move |_| clear_form()>"清空"</button>
                    </div>
                    <div class="knowledge-form-grid">
                        <WorkbenchSelect
                            label="类型"
                            options=Signal::derive(|| vec![SelectOption::new("term", "企业术语"), SelectOption::new("field", "字段目录"), SelectOption::new("policy", "制度政策")])
                            value=source_type
                            disabled=busy
                            placeholder=Signal::derive(|| "请选择类型".to_string())
                            on_change=Callback::new(move |value| source_type.set(value))
                        />
                        <label><span>"名称"</span><input maxlength="160" prop:value=name on:input=move |event| name.set(event_target_value(&event)) /></label>
                        <label><span>"版本"</span><input maxlength="80" placeholder="例如 2026.1" prop:value=version_label on:input=move |event| version_label.set(event_target_value(&event)) /></label>
                        <label class="knowledge-form-wide"><span>"说明"</span><input maxlength="4000" prop:value=description on:input=move |event| description.set(event_target_value(&event)) /></label>
                        <WorkbenchDatePicker label="生效日期" value=valid_from disabled=busy on_change=Callback::new(move |value| valid_from.set(value)) />
                        <WorkbenchDatePicker label="失效日期" value=valid_until disabled=busy on_change=Callback::new(move |value| valid_until.set(value)) />
                        <label class="knowledge-form-wide"><span>"来源地址"</span><input maxlength="2000" placeholder="制度页面、文档或数据字典地址" prop:value=source_uri on:input=move |event| source_uri.set(event_target_value(&event)) /></label>
                        <label class="knowledge-form-wide"><span>"知识内容"</span><textarea maxlength="50000" rows="7" prop:value=content on:input=move |event| content.set(event_target_value(&event))></textarea></label>
                    </div>
                    <button class="button button-primary" on:click=save_source disabled=move || busy.get() || name.get().trim().is_empty() || content.get().trim().is_empty()>{move || if busy.get() { "处理中..." } else if selected_id.get().is_some() { "保存修改" } else { "创建知识源" }}</button>
                    <Show when=move || error.get().is_some()><strong class="knowledge-error">{move || error.get().unwrap_or_default()}</strong></Show>
                </div>
            </div>
        </section>
    }
}
