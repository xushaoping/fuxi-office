use leptos::prelude::*;
use leptos::task::spawn_local;
use std::collections::HashSet;

use crate::api;
use crate::components::common::{SelectOption, WorkbenchDatePicker, WorkbenchSelect};
use crate::models::{
    DecisionProject, KnowledgeIndexStatus, KnowledgeSearchChunkDiagnostic,
    KnowledgeSearchDiagnostics, KnowledgeSource, PatternSearchDiagnostics, SaveKnowledgeSource,
};

const MAX_IMPORTED_TEXT_BYTES: f64 = 50_000.0;
const MAX_BATCH_IMPORT_FILES: usize = 10;

fn is_supported_text_file(filename: &str) -> bool {
    matches!(
        filename
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "txt" | "md" | "markdown" | "csv" | "json" | "yaml" | "yml" | "xml"
    )
}

fn document_name(filename: &str) -> String {
    filename
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .filter(|stem| !stem.trim().is_empty())
        .unwrap_or(filename)
        .to_string()
}

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

fn knowledge_source_matches_filter(
    source: &KnowledgeSource,
    filter: &str,
    is_attached: bool,
) -> bool {
    match filter {
        "all" => true,
        "attached" => is_attached,
        "archived" => source.status == "archived",
        source_type => source.source_type == source_type,
    }
}

fn knowledge_index_label(status: Option<&KnowledgeIndexStatus>, source_id: i64) -> String {
    let Some(status) = status else {
        return "索引状态读取中".into();
    };
    let Some(source) = status
        .sources
        .iter()
        .find(|item| item.source_id == source_id)
    else {
        return "未生成分块".into();
    };
    if source.chunk_count == 0 {
        return "未生成分块".into();
    }
    if !status.vector_available {
        return format!(
            "{} 个分块 · 关键词模式（pgvector 不可用）",
            source.chunk_count
        );
    }
    if !status.embedding_enabled {
        return format!("{} 个分块 · Embedding 未启用", source.chunk_count);
    }
    if source.current_model_chunk_count < source.chunk_count {
        return format!(
            "当前模型向量 {}/{} · 已有向量 {}",
            source.current_model_chunk_count, source.chunk_count, source.embedded_chunk_count
        );
    }
    format!(
        "当前模型向量就绪 {}/{}",
        source.current_model_chunk_count, source.chunk_count
    )
}

fn search_debug_excerpt(value: &str) -> String {
    let excerpt = value.chars().take(240).collect::<String>();
    if value.chars().count() > 240 {
        format!("{excerpt}…")
    } else {
        excerpt
    }
}

fn search_debug_chunk_label(chunk: &KnowledgeSearchChunkDiagnostic) -> String {
    format!(
        "{} · {} · #{}",
        chunk.source_name, chunk.source_type, chunk.id
    )
}

fn search_debug_vector_empty_label(status: &str) -> &'static str {
    match status {
        "disabled" => "Embedding 未启用",
        "pgvector_unavailable" => "pgvector 向量列不可用",
        "provider_error" => "Embedding 服务调用失败，请检查后端日志",
        "query_error" => "向量查询失败，请检查后端日志",
        "not_run" => "当前没有可检索的知识源",
        _ => "无匹配结果",
    }
}

#[component]
pub fn KnowledgeLibraryPanel(
    token: RwSignal<Option<String>>,
    current_project: ReadSignal<Option<DecisionProject>>,
) -> impl IntoView {
    let sources = RwSignal::new(Vec::<KnowledgeSource>::new());
    let index_status = RwSignal::new(None::<KnowledgeIndexStatus>);
    let index_error = RwSignal::new(None::<String>);
    let index_refresh = RwSignal::new(0_u64);
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
    let importing = RwSignal::new(false);
    let batch_import_summary = RwSignal::new(None::<String>);
    let batch_import_progress = RwSignal::new(None::<String>);
    let batch_import_error = RwSignal::new(None::<String>);
    let reindexing_source = RwSignal::new(None::<i64>);
    let archive_confirmation = RwSignal::new(None::<KnowledgeSource>);
    let archive_error = RwSignal::new(None::<String>);
    let source_action_error = RwSignal::new(None::<String>);
    let error = RwSignal::new(None::<String>);
    let debug_query = RwSignal::new(String::new());
    let debug_kind = RwSignal::new("knowledge".to_string());
    let debug_open = RwSignal::new(false);
    let debug_busy = RwSignal::new(false);
    let debug_error = RwSignal::new(None::<String>);
    let knowledge_diagnostics = RwSignal::new(None::<KnowledgeSearchDiagnostics>);
    let pattern_diagnostics = RwSignal::new(None::<PatternSearchDiagnostics>);
    let debug_generation = RwSignal::new(0_u64);
    let source_load_generation = RwSignal::new(0_u64);

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
        index_refresh.get();
        let Some(token_value) = token.get() else {
            source_load_generation.update(|generation| *generation = generation.wrapping_add(1));
            sources.set(Vec::new());
            index_status.set(None);
            index_error.set(None);
            attached_ids.set(HashSet::new());
            busy.set(false);
            return;
        };
        let project_id = current_project.get().map(|project| project.id);
        source_load_generation.update(|generation| *generation = generation.wrapping_add(1));
        let request_generation = source_load_generation.get();
        busy.set(true);
        error.set(None);
        index_status.set(None);
        index_error.set(None);
        spawn_local(async move {
            let source_result = api::list_knowledge_sources(&token_value).await;
            let index_result = api::get_knowledge_index_status(&token_value).await;
            let attachment_result = if let Some(project_id) = project_id {
                api::list_project_knowledge_sources(&token_value, project_id)
                    .await
                    .map(|items| {
                        items
                            .into_iter()
                            .map(|item| item.id)
                            .collect::<HashSet<_>>()
                    })
            } else {
                Ok(HashSet::new())
            };
            if source_load_generation.get() != request_generation
                || current_project.get().map(|project| project.id) != project_id
            {
                return;
            }
            match source_result {
                Ok(items) => sources.set(items),
                Err(message) => error.set(Some(message)),
            }
            match index_result {
                Ok(status) => index_status.set(Some(status)),
                Err(message) => index_error.set(Some(message)),
            }
            match attachment_result {
                Ok(ids) => attached_ids.set(ids),
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    });

    Effect::new(move |_| {
        current_project.get();
        debug_generation.update(|generation| *generation = generation.wrapping_add(1));
        knowledge_diagnostics.set(None);
        pattern_diagnostics.set(None);
        debug_error.set(None);
        debug_busy.set(false);
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
                    index_refresh.update(|generation| *generation = generation.wrapping_add(1));
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
                    index_refresh.update(|generation| *generation = generation.wrapping_add(1));
                    if selected_id.get() == Some(source_id) {
                        clear_form();
                    }
                    archive_confirmation.set(None);
                }
                Err(message) => archive_error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    let restore_source = move |source_id: i64| {
        let Some(token_value) = token.get() else {
            source_action_error.set(Some("登录状态已失效，请重新登录".into()));
            return;
        };
        let Some(source) = sources
            .get_untracked()
            .into_iter()
            .find(|source| source.id == source_id)
        else {
            source_action_error.set(Some("知识源已不存在，请刷新列表".into()));
            return;
        };
        let payload = SaveKnowledgeSource {
            source_type: source.source_type.clone(),
            name: source.name.clone(),
            description: source.description.clone(),
            content: source.content.clone(),
            version_label: source.version_label.clone(),
            source_uri: source.source_uri.clone(),
            valid_from: source.valid_from,
            valid_until: source.valid_until,
            status: "active".into(),
        };
        busy.set(true);
        source_action_error.set(None);
        spawn_local(async move {
            match api::update_knowledge_source(&token_value, source_id, &payload).await {
                Ok(saved) => {
                    sources.update(|items| {
                        if let Some(item) = items.iter_mut().find(|item| item.id == saved.id) {
                            *item = saved;
                        }
                    });
                    index_refresh.update(|generation| *generation = generation.wrapping_add(1));
                }
                Err(message) => source_action_error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    let reindex_source = move |source_id: i64| {
        let Some(token_value) = token.get() else {
            return;
        };
        if busy.get_untracked() || reindexing_source.get_untracked().is_some() {
            return;
        }
        reindexing_source.set(Some(source_id));
        error.set(None);
        spawn_local(async move {
            match api::reindex_knowledge_source(&token_value, source_id).await {
                Ok(()) => {
                    index_refresh.update(|generation| *generation = generation.wrapping_add(1));
                }
                Err(message) => error.set(Some(message)),
            }
            reindexing_source.set(None);
        });
    };

    let import_document = move |event: web_sys::Event| {
        let input = event_target::<web_sys::HtmlInputElement>(&event);
        let Some(file) = input.files().and_then(|files| files.item(0)) else {
            return;
        };
        input.set_value("");
        let filename = file.name();
        if !is_supported_text_file(&filename) {
            error.set(Some(
                "仅支持 TXT、Markdown、CSV、JSON、YAML 和 XML 文件".into(),
            ));
            return;
        }
        if file.size() > MAX_IMPORTED_TEXT_BYTES {
            error.set(Some("导入文件不能超过 50,000 字节".into()));
            return;
        }
        if importing.get_untracked() {
            return;
        }
        importing.set(true);
        error.set(None);
        spawn_local(async move {
            let result = wasm_bindgen_futures::JsFuture::from(file.text())
                .await
                .ok()
                .and_then(|value| value.as_string());
            match result {
                Some(text) if text.len() <= MAX_IMPORTED_TEXT_BYTES as usize => {
                    content.set(text);
                    if name.get().trim().is_empty() {
                        name.set(document_name(&filename));
                    }
                }
                Some(_) => error.set(Some("导入正文超过 50,000 字节".into())),
                None => error.set(Some("读取文件失败，请确认文件是有效文本".into())),
            }
            importing.set(false);
        });
    };

    let import_documents = move |event: web_sys::Event| {
        let input = event_target::<web_sys::HtmlInputElement>(&event);
        let Some(file_list) = input.files() else {
            return;
        };
        let files = (0..file_list.length())
            .filter_map(|index| file_list.item(index))
            .collect::<Vec<_>>();
        input.set_value("");
        if files.is_empty() {
            return;
        }
        if files.len() > MAX_BATCH_IMPORT_FILES {
            batch_import_error.set(Some(format!(
                "每批最多导入 {MAX_BATCH_IMPORT_FILES} 个文件"
            )));
            return;
        }
        if busy.get_untracked() || importing.get_untracked() {
            return;
        }
        let Some(token_value) = token.get() else {
            batch_import_error.set(Some("登录状态已失效，请重新登录".into()));
            return;
        };
        for file in &files {
            let filename = file.name();
            if !is_supported_text_file(&filename) {
                batch_import_error.set(Some(format!("不支持的文件格式：{filename}")));
                return;
            }
            if file.size() > MAX_IMPORTED_TEXT_BYTES {
                batch_import_error.set(Some(format!("文件超过 50,000 字节：{filename}")));
                return;
            }
            if document_name(&filename).chars().count() > 160 {
                batch_import_error.set(Some(format!("文件名过长：{filename}")));
                return;
            }
        }

        let selected_type = source_type.get();
        let selected_version = version_label.get();
        let total = files.len();
        busy.set(true);
        importing.set(true);
        error.set(None);
        batch_import_summary.set(None);
        batch_import_error.set(None);
        batch_import_progress.set(Some(format!("准备导入 {total} 个文件…")));
        spawn_local(async move {
            let mut imported = Vec::new();
            let mut failures = Vec::new();
            for (index, file) in files.into_iter().enumerate() {
                let filename = file.name();
                batch_import_progress.set(Some(format!(
                    "正在导入 {}/{}：{}",
                    index + 1,
                    total,
                    filename
                )));
                let text = wasm_bindgen_futures::JsFuture::from(file.text())
                    .await
                    .ok()
                    .and_then(|value| value.as_string());
                let Some(content) = text.filter(|content| {
                    !content.trim().is_empty() && content.len() <= MAX_IMPORTED_TEXT_BYTES as usize
                }) else {
                    failures.push(format!("{filename}（读取失败或正文为空/过大）"));
                    continue;
                };
                let payload = SaveKnowledgeSource {
                    source_type: selected_type.clone(),
                    name: document_name(&filename),
                    description: String::new(),
                    content,
                    version_label: selected_version.clone(),
                    source_uri: String::new(),
                    valid_from: None,
                    valid_until: None,
                    status: "active".into(),
                };
                match api::create_knowledge_source(&token_value, &payload).await {
                    Ok(source) => imported.push(source),
                    Err(message) => failures.push(format!("{filename}（{message}）")),
                }
            }

            let imported_count = imported.len();
            if imported_count > 0 {
                sources.update(|items| {
                    for source in imported.into_iter().rev() {
                        items.insert(0, source);
                    }
                });
                batch_import_summary.set(Some(format!(
                    "批量导入完成：成功 {imported_count} / {total} 个"
                )));
            } else {
                batch_import_summary.set(Some(format!("批量导入未成功：0 / {total} 个")));
            }
            if !failures.is_empty() {
                batch_import_error.set(Some(format!(
                    "{} 个文件导入失败：{}",
                    failures.len(),
                    failures.into_iter().take(3).collect::<Vec<_>>().join("；")
                )));
            }
            batch_import_progress.set(None);
            busy.set(false);
            importing.set(false);
            if imported_count > 0 {
                index_refresh.update(|generation| *generation = generation.wrapping_add(1));
            }
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

    let run_search_diagnostics = move |_| {
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            debug_error.set(Some("请先选择决策项目".into()));
            return;
        };
        let query_value = debug_query.get().trim().to_string();
        if query_value.is_empty() || query_value.chars().count() > 500 {
            debug_error.set(Some("请输入 1–500 个字符的检索词".into()));
            return;
        }
        debug_generation.update(|generation| *generation = generation.wrapping_add(1));
        let request_generation = debug_generation.get();
        let project_id = project.id;
        let selected_kind = debug_kind.get();
        debug_busy.set(true);
        debug_error.set(None);
        knowledge_diagnostics.set(None);
        pattern_diagnostics.set(None);
        spawn_local(async move {
            if debug_generation.get() != request_generation
                || current_project.get().map(|item| item.id) != Some(project_id)
            {
                return;
            }
            if selected_kind == "knowledge" {
                let result =
                    api::debug_project_knowledge_search(&token_value, project_id, &query_value)
                        .await;
                if debug_generation.get() == request_generation
                    && current_project.get().map(|item| item.id) == Some(project_id)
                {
                    match result {
                        Ok(diagnostics) => knowledge_diagnostics.set(Some(diagnostics)),
                        Err(message) => debug_error.set(Some(message)),
                    }
                    debug_busy.set(false);
                }
            } else {
                let result =
                    api::debug_pattern_search(&token_value, project_id, &query_value).await;
                if debug_generation.get() == request_generation
                    && current_project.get().map(|item| item.id) == Some(project_id)
                {
                    match result {
                        Ok(diagnostics) => pattern_diagnostics.set(Some(diagnostics)),
                        Err(message) => debug_error.set(Some(message)),
                    }
                    debug_busy.set(false);
                }
            }
        });
    };

    view! {
        <section class="knowledge-library-panel" aria-label="企业知识库">
            <div class="knowledge-library-heading">
                <div>
                    <span class="yaml-kicker">"GOVERNED KNOWLEDGE"</span>
                    <strong>"企业知识库"</strong>
                    <small>{move || format!("{} 个知识源 · 当前项目已绑定 {} 个", sources.get().len(), attached_ids.get().len())}</small>
                    <small class="knowledge-index-summary">
                        {move || index_status.get().map(|status| {
                            if status.vector_available {
                                format!("Embedding {} · 当前模型 {}", if status.embedding_enabled { "已启用" } else { "未启用" }, status.current_model)
                            } else {
                                "pgvector 不可用 · 当前使用关键词检索".into()
                            }
                        }).unwrap_or_else(|| "正在读取索引状态".into())}
                    </small>
                    <Show when=move || index_error.get().is_some()>
                        <small class="knowledge-index-error">{move || index_error.get().unwrap_or_default()}</small>
                    </Show>
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
                        <For each=move || vec![("all", "全部"), ("attached", "已绑定"), ("archived", "已归档"), ("term", "术语"), ("field", "字段"), ("policy", "制度")] key=|item| item.0 let:item>
                            <button class=move || if filter.get() == item.0 { "knowledge-filter is-active" } else { "knowledge-filter" } on:click=move |_| filter.set(item.0.into())>{item.1}</button>
                        </For>
                    </div>
                    <button
                        class="button button-secondary knowledge-diagnostics-trigger"
                        type="button"
                        aria-haspopup="dialog"
                        on:click=move |_| debug_open.set(true)
                    >
                        "检索诊断"
                    </button>
                </div>
            </div>
            <div class="knowledge-library-body">
                <div class="knowledge-source-browser">
                    <div class="resource-column-heading">
                        <strong>"知识目录"</strong>
                        <label class="knowledge-import-button knowledge-batch-import-button" title="批量导入文本知识源，最多 10 个">
                            <input
                                class="knowledge-import-input"
                                type="file"
                                multiple
                                accept=".txt,.md,.markdown,.csv,.json,.yaml,.yml,.xml,text/plain,text/markdown"
                                disabled=move || busy.get() || importing.get()
                                on:change=import_documents
                            />
                            <span>{move || if importing.get() { "导入中..." } else { "批量导入" }}</span>
                        </label>
                        <button class="button button-quiet" on:click=move |_| clear_form() disabled=move || importing.get() || busy.get()>"新建"</button>
                    </div>
                    <Show when=move || batch_import_summary.get().is_some()>
                        <small class="knowledge-batch-import-summary">{move || batch_import_summary.get().unwrap_or_default()}</small>
                    </Show>
                    <Show when=move || batch_import_progress.get().is_some()>
                        <small class="knowledge-batch-import-progress" role="status">{move || batch_import_progress.get().unwrap_or_default()}</small>
                    </Show>
                    <Show when=move || batch_import_error.get().is_some()>
                        <small class="knowledge-batch-import-error" role="alert">{move || batch_import_error.get().unwrap_or_default()}</small>
                    </Show>
                    <Show when=move || source_action_error.get().is_some()>
                        <small class="knowledge-index-error" role="alert">{move || source_action_error.get().unwrap_or_default()}</small>
                    </Show>
                    <div class="knowledge-source-list">
                    <Show when=move || busy.get() && sources.get().is_empty()>
                        <div class="knowledge-empty">"正在读取知识库..."</div>
                    </Show>
                    <For
                        each=move || {
                            let selected = filter.get();
                            let search = query.get().trim().to_lowercase();
                            sources.get().into_iter().filter(|source| {
                                knowledge_source_matches_filter(
                                    source,
                                    &selected,
                                    attached_ids.get().contains(&source.id),
                                )
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
                            let source_for_archive = source.clone();
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
                                    <small class="knowledge-index-status">
                                        {move || knowledge_index_label(index_status.get().as_ref(), source_id)}
                                    </small>
                                    <div class="knowledge-source-actions">
                                        <label title="绑定后，当前有效内容会进入该项目的智能上下文">
                                            <input type="checkbox" prop:checked=move || attached_ids.get().contains(&source_id) on:change=move |event| toggle_attachment(source_id, event_target_checked(&event)) disabled=archived />
                                            <span>"绑定当前项目"</span>
                                        </label>
                                        <button
                                            class="button button-quiet"
                                            type="button"
                                            on:click=move |_| reindex_source(source_id)
                                            disabled=move || archived || busy.get() || reindexing_source.get().is_some()
                                        >
                                            {move || if reindexing_source.get() == Some(source_id) { "重建中..." } else { "重建索引" }}
                                        </button>
                                        <button class="button button-quiet" on:click=move |_| edit_source(source_for_edit.clone())>"编辑"</button>
                                        <Show when=move || archived>
                                            <button
                                                class="button button-quiet"
                                                type="button"
                                                on:click=move |_| restore_source(source_id)
                                                disabled=move || busy.get()
                                            >"恢复"</button>
                                        </Show>
                                        <button
                                            class="button button-quiet"
                                            type="button"
                                            on:click=move |_| {
                                                archive_error.set(None);
                                                archive_confirmation.set(Some(source_for_archive.clone()));
                                            }
                                            disabled=move || busy.get() || archived
                                        >"归档"</button>
                                    </div>
                                </article>
                            }
                        }
                    </For>
                    <Show when=move || {
                        let selected = filter.get();
                        let search = query.get().trim().to_lowercase();
                        !busy.get() && !sources.get().iter().any(|source| {
                            knowledge_source_matches_filter(
                                source,
                                &selected,
                                attached_ids.get().contains(&source.id),
                            )
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
                        <button class="button button-quiet" on:click=move |_| clear_form() disabled=move || importing.get() || busy.get()>"清空"</button>
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
                        <div class="knowledge-form-wide knowledge-content-field">
                            <div class="knowledge-content-heading">
                                <label for="knowledge-content">"知识内容"</label>
                                <label class="knowledge-import-button" title="导入 TXT、Markdown、CSV、JSON、YAML 或 XML">
                                    <input
                                        class="knowledge-import-input"
                                        type="file"
                                        accept=".txt,.md,.markdown,.csv,.json,.yaml,.yml,.xml,text/plain,text/markdown"
                                        disabled=move || importing.get() || busy.get()
                                        on:change=import_document
                                    />
                                    <span>{move || if importing.get() { "读取中..." } else { "导入文本" }}</span>
                                </label>
                            </div>
                            <textarea id="knowledge-content" maxlength="50000" aria-describedby="knowledge-content-size" rows="6" prop:value=content on:input=move |event| content.set(event_target_value(&event))></textarea>
                            <small
                                id="knowledge-content-size"
                                class=move || {
                                    if content.get().len() > MAX_IMPORTED_TEXT_BYTES as usize {
                                        "knowledge-content-size is-over-limit"
                                    } else {
                                        "knowledge-content-size"
                                    }
                                }
                                role="status"
                            >
                                {move || format!("{} / 50,000 字节", content.get().len())}
                            </small>
                        </div>
                    </div>
                    <button class="button button-primary" on:click=save_source disabled=move || {
                        busy.get()
                            || importing.get()
                            || name.get().trim().is_empty()
                            || content.get().trim().is_empty()
                            || (content.get().len() > MAX_IMPORTED_TEXT_BYTES as usize)
                    }>{move || if busy.get() { "处理中..." } else if importing.get() { "读取文档..." } else if selected_id.get().is_some() { "保存修改" } else { "创建知识源" }}</button>
                    <Show when=move || error.get().is_some()><strong class="knowledge-error">{move || error.get().unwrap_or_default()}</strong></Show>
                </div>
            </div>
            <Show when=move || archive_confirmation.get().is_some()>
                <div
                    class="knowledge-confirm-backdrop"
                    aria-hidden="true"
                    on:click=move |_| {
                        if !busy.get() {
                            archive_confirmation.set(None);
                        }
                    }
                ></div>
                <section
                    class="knowledge-confirm-dialog"
                    role="alertdialog"
                    aria-modal="true"
                    aria-labelledby="knowledge-archive-title"
                    aria-describedby="knowledge-archive-description"
                    tabindex="-1"
                    autofocus
                    on:keydown=move |event: leptos::ev::KeyboardEvent| {
                        if event.key() == "Escape" && !busy.get() {
                            archive_confirmation.set(None);
                        }
                    }
                >
                    <span class="yaml-kicker">"SOURCE LIFECYCLE"</span>
                    <h2 id="knowledge-archive-title">"归档知识源"</h2>
                    <strong>{move || archive_confirmation.get().map(|source| source.name).unwrap_or_default()}</strong>
                    <p id="knowledge-archive-description">
                        "归档后该知识源将不参与检索，正文和项目绑定关系会保留。之后可从目录恢复；是否进入检索仍取决于有效期。"
                    </p>
                    <Show when=move || archive_error.get().is_some()>
                        <small class="knowledge-error" role="alert">{move || archive_error.get().unwrap_or_default()}</small>
                    </Show>
                    <div class="knowledge-confirm-actions">
                        <button
                            class="button button-quiet"
                            type="button"
                            on:click=move |_| archive_confirmation.set(None)
                            disabled=busy
                        >"取消"</button>
                        <button
                            class="button button-danger"
                            type="button"
                            on:click=move |_| {
                                if let Some(source) = archive_confirmation.get() {
                                    archive_source(source.id);
                                }
                            }
                            disabled=busy
                        >{move || if busy.get() { "归档中..." } else { "确认归档" }}</button>
                    </div>
                </section>
            </Show>
            <Show when=move || debug_open.get()>
                <div
                    class="knowledge-diagnostics-backdrop"
                    aria-hidden="true"
                    on:click=move |_| debug_open.set(false)
                ></div>
                <aside
                    class="knowledge-diagnostics-drawer"
                    role="dialog"
                    aria-modal="true"
                    aria-labelledby="knowledge-search-debug-title"
                    on:keydown=move |event: leptos::ev::KeyboardEvent| {
                        if event.key() == "Escape" {
                            debug_open.set(false);
                        }
                    }
                >
                    <header class="knowledge-diagnostics-drawer-heading">
                        <div>
                            <span class="yaml-kicker">"RETRIEVAL INSPECTOR"</span>
                            <strong id="knowledge-search-debug-title">"检索诊断"</strong>
                            <small>{move || current_project.get().map(|project| project.name).unwrap_or_else(|| "请先选择项目".into())}</small>
                        </div>
                        <button
                            class="knowledge-diagnostics-close"
                            type="button"
                            aria-label="关闭检索诊断"
                            title="关闭"
                            autofocus
                            on:click=move |_| debug_open.set(false)
                        >
                            "×"
                        </button>
                    </header>
                    <div class="knowledge-diagnostics-drawer-body">
            <section class="knowledge-search-debug" aria-label="检索阶段结果">
                <div class="knowledge-search-debug-heading">
                    <form class="knowledge-search-debug-form" on:submit=move |event| {
                        event.prevent_default();
                        run_search_diagnostics(());
                    }>
                        <input
                            type="search"
                            maxlength="500"
                            placeholder="输入查询词"
                            prop:value=debug_query
                            on:input=move |event| debug_query.set(event_target_value(&event))
                            disabled=move || current_project.get().is_none() || debug_busy.get()
                        />
                        <button
                            class="button button-secondary"
                            type="submit"
                            disabled=move || current_project.get().is_none() || debug_query.get().trim().is_empty() || debug_busy.get()
                        >
                            {move || if debug_busy.get() { "检索中…" } else { "运行诊断" }}
                        </button>
                    </form>
                </div>
                <Show when=move || debug_error.get().is_some()>
                    <div class="knowledge-error" role="alert">{move || debug_error.get().unwrap_or_default()}</div>
                </Show>
                <div class="knowledge-search-debug-mode" role="group" aria-label="检索类型">
                    <button class=move || if debug_kind.get() == "knowledge" { "knowledge-search-debug-mode-active" } else { "" } on:click=move |_| debug_kind.set("knowledge".into()) disabled=move || debug_busy.get()>"知识块"</button>
                    <button class=move || if debug_kind.get() == "patterns" { "knowledge-search-debug-mode-active" } else { "" } on:click=move |_| debug_kind.set("patterns".into()) disabled=move || debug_busy.get()>"模式"</button>
                </div>
                <Show when=move || {
                    debug_kind.get() == "knowledge"
                        && knowledge_diagnostics
                            .get()
                            .and_then(|items| items.minimum_vector_similarity)
                            .is_some()
                }>
                    <small class="knowledge-search-debug-threshold">
                        {move || knowledge_diagnostics.get().and_then(|items| items.minimum_vector_similarity).map(|score| format!("向量最低相似度 {:.2} · 低于此值的结果不会进入融合", score)).unwrap_or_default()}
                    </small>
                </Show>
                <Show
                    when=move || debug_busy.get()
                    fallback=move || view! {
                        <Show
                            when=move || knowledge_diagnostics.get().is_some() || pattern_diagnostics.get().is_some()
                            fallback=|| view! { <p class="knowledge-search-debug-empty">"尚未运行诊断"</p> }
                        >
                            <Show when=move || debug_kind.get() == "knowledge" fallback=move || view! {
                                <div class="knowledge-search-debug-grid">
                                    {let results = move || pattern_diagnostics.get().map(|items| items.keyword_candidate_ids).unwrap_or_default();
                                    view! { <div class="knowledge-search-debug-stage"><h3>"关键词"</h3>
                                        <For each=move || { results().into_iter().enumerate().collect::<Vec<_>>() } key=|row| row.1 let:row>
                                            <p class="knowledge-search-debug-id">{format!("排名 {} · 模式 #{}", row.0 + 1, row.1)}</p>
                                        </For>
                                        <Show when=move || pattern_diagnostics.get().is_some_and(|items| items.keyword_candidate_ids.is_empty())><p class="knowledge-search-debug-empty">"无匹配结果"</p></Show>
                                    </div> }}
                                    {let results = move || pattern_diagnostics.get().map(|items| items.vector_candidates).unwrap_or_default();
                                    view! { <div class="knowledge-search-debug-stage"><h3>"向量"</h3>
                                        <For each=move || { results().into_iter().enumerate().collect::<Vec<_>>() } key=|row| row.1.0 let:row>
                                            <p class="knowledge-search-debug-id">{format!("排名 {} · 模式 #{} · 相似度 {:.4}", row.0 + 1, row.1.0, row.1.1)}</p>
                                        </For>
                                        <Show when=move || pattern_diagnostics.get().is_some_and(|items| items.vector_candidates.is_empty())><p class="knowledge-search-debug-empty">"无匹配结果"</p></Show>
                                    </div> }}
                                    {let results = move || pattern_diagnostics.get().map(|items| items.rrf_candidate_ids).unwrap_or_default();
                                    view! { <div class="knowledge-search-debug-stage"><h3>"RRF 融合"</h3>
                                        <For each=move || { results().into_iter().enumerate().collect::<Vec<_>>() } key=|row| row.1 let:row>
                                            <p class="knowledge-search-debug-id">{format!("排名 {} · 模式 #{}", row.0 + 1, row.1)}</p>
                                        </For>
                                        <Show when=move || pattern_diagnostics.get().is_some_and(|items| items.rrf_candidate_ids.is_empty())><p class="knowledge-search-debug-empty">"无匹配结果"</p></Show>
                                    </div> }}
                                </div>
                            }>
                                <div class="knowledge-search-debug-grid">
                                    {let results = move || knowledge_diagnostics.get().map(|items| items.keyword_results).unwrap_or_default();
                                    view! { <div class="knowledge-search-debug-stage"><h3>"关键词"</h3>
                                        <For each=move || { results().into_iter().enumerate().collect::<Vec<_>>() } key=|row| row.1.id let:row>
                                            <article class="knowledge-search-debug-result"><strong>{search_debug_chunk_label(&row.1)}</strong><small>{format!("排名 {}", row.0 + 1)}</small><p>{search_debug_excerpt(&row.1.content)}</p></article>
                                        </For>
                                        <Show when=move || knowledge_diagnostics.get().is_some_and(|items| items.keyword_results.is_empty())><p class="knowledge-search-debug-empty">"无匹配结果"</p></Show>
                                    </div> }}
                                    {let results = move || knowledge_diagnostics.get().map(|items| items.vector_results).unwrap_or_default();
                                    view! { <div class="knowledge-search-debug-stage"><h3>"向量"</h3>
                                        <For each=move || { results().into_iter().enumerate().collect::<Vec<_>>() } key=|row| row.1.id let:row>
                                            <article class="knowledge-search-debug-result"><strong>{search_debug_chunk_label(&row.1)}</strong><small>{format!("排名 {}{}", row.0 + 1, row.1.similarity.map(|score| format!(" · 相似度 {score:.4}")).unwrap_or_default())}</small><p>{search_debug_excerpt(&row.1.content)}</p></article>
                                        </For>
                                        <Show when=move || knowledge_diagnostics.get().is_some_and(|items| items.vector_results.is_empty())><p class="knowledge-search-debug-empty">{move || knowledge_diagnostics.get().map(|items| search_debug_vector_empty_label(&items.vector_status)).unwrap_or("无匹配结果")}</p></Show>
                                    </div> }}
                                    {let results = move || knowledge_diagnostics.get().map(|items| items.fused_results).unwrap_or_default();
                                    view! { <div class="knowledge-search-debug-stage"><h3>"RRF 融合"</h3>
                                        <For each=move || { results().into_iter().enumerate().collect::<Vec<_>>() } key=|row| row.1.id let:row>
                                            <article class="knowledge-search-debug-result"><strong>{search_debug_chunk_label(&row.1)}</strong><small>{format!("排名 {}{}", row.0 + 1, row.1.similarity.map(|score| format!(" · 相似度 {score:.4}")).unwrap_or_default())}</small><p>{search_debug_excerpt(&row.1.content)}</p></article>
                                        </For>
                                        <Show when=move || knowledge_diagnostics.get().is_some_and(|items| items.fused_results.is_empty())><p class="knowledge-search-debug-empty">"无匹配结果"</p></Show>
                                    </div> }}
                                </div>
                            </Show>
                        </Show>
                    }
                >
                    <p class="knowledge-search-debug-empty">"正在读取各检索阶段..."</p>
                </Show>
            </section>
                    </div>
                </aside>
            </Show>
        </section>
    }
}
