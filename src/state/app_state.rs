use gloo_storage::{LocalStorage, Storage};
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::models::{KnowledgeCitation, VersionChangeReview};

static TOAST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    Dark,
    #[default]
    Light,
}

impl Theme {
    pub fn class(self) -> &'static str {
        match self {
            Self::Dark => "theme-dark",
            Self::Light => "theme-light",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }

    pub fn toggle_label(self) -> &'static str {
        match self {
            Self::Dark => "切换到浅色主题",
            Self::Light => "切换到深色主题",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkspaceView {
    #[default]
    Discovery,
    Model,
    Validation,
    Services,
    Analytics,
    Versions,
    Collaboration,
    Resources,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDraftSnapshot {
    pub project_id: i64,
    #[serde(default)]
    pub base_version_id: Option<i64>,
    pub prompt: String,
    pub clarification_answers: BTreeMap<String, String>,
    #[serde(default)]
    pub pending_review: Option<VersionChangeReview>,
    #[serde(default)]
    pub pending_candidate_id: Option<i64>,
    #[serde(default)]
    pub pending_knowledge_citations: Vec<KnowledgeCitation>,
    #[serde(default)]
    pub review_confirmed: bool,
    #[serde(default)]
    pub dsl: Option<String>,
    #[serde(default)]
    pub source_dsl: String,
    #[serde(default)]
    pub xml: Option<String>,
    #[serde(default)]
    pub workspace_view: String,
    #[serde(default)]
    pub modify_mode: bool,
    #[serde(default)]
    pub version_mismatch: bool,
}

fn workspace_draft_key(project_id: i64) -> String {
    format!("fuxi-dmn:workspace-draft:{project_id}")
}

#[derive(Clone, Copy, Debug)]
pub struct AuthState {
    pub token: RwSignal<Option<String>>,
    pub user: RwSignal<Option<String>>,
}

#[derive(Clone, Copy, Debug)]
pub struct UiState {
    pub theme: RwSignal<Theme>,
    pub sidebar_open: RwSignal<bool>,
    pub active_workspace: RwSignal<WorkspaceView>,
    pub workspace_dirty: RwSignal<bool>,
    pub workspace_draft_storage_error: RwSignal<Option<String>>,
    pub active_project_id: RwSignal<Option<i64>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToastLevel {
    Success,
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToastMessage {
    pub id: String,
    pub level: ToastLevel,
    pub title: String,
    pub message: String,
    pub duration: Option<u64>,
}

#[derive(Clone, Copy, Debug)]
pub struct ToastState {
    pub messages: RwSignal<Vec<ToastMessage>>,
}

#[derive(Clone, Copy, Debug)]
pub struct AppState {
    pub auth: AuthState,
    pub ui: UiState,
    pub toast: ToastState,
}

impl AppState {
    pub fn new() -> Self {
        let theme = LocalStorage::get("theme").unwrap_or_default();
        Self {
            auth: AuthState {
                token: RwSignal::new(LocalStorage::get("auth_token").ok()),
                user: RwSignal::new(None),
            },
            ui: UiState {
                theme: RwSignal::new(theme),
                sidebar_open: RwSignal::new(true),
                active_workspace: RwSignal::new(WorkspaceView::default()),
                workspace_dirty: RwSignal::new(false),
                workspace_draft_storage_error: RwSignal::new(None),
                active_project_id: RwSignal::new(LocalStorage::get("active_project_id").ok()),
            },
            toast: ToastState {
                messages: RwSignal::new(Vec::new()),
            },
        }
    }

    pub fn set_theme(&self, theme: Theme) {
        self.ui.theme.set(theme);
        let _ = LocalStorage::set("theme", theme);
    }

    pub fn load_workspace_draft(&self, project_id: i64) -> Option<WorkspaceDraftSnapshot> {
        let key = workspace_draft_key(project_id);
        LocalStorage::get(&key).ok()
    }

    pub fn save_workspace_draft(&self, snapshot: &WorkspaceDraftSnapshot) -> bool {
        let key = workspace_draft_key(snapshot.project_id);
        let saved = LocalStorage::set(&key, snapshot).is_ok();
        self.ui.workspace_draft_storage_error.set(if saved {
            None
        } else {
            Some("本地草稿无法保存，浏览器存储空间可能不足".into())
        });
        saved
    }

    pub fn clear_workspace_draft(&self, project_id: i64) {
        let key = workspace_draft_key(project_id);
        let _ = LocalStorage::delete(&key);
        self.ui.workspace_draft_storage_error.set(None);
    }

    pub fn set_active_project(&self, project_id: Option<i64>) {
        self.ui.active_project_id.set(project_id);
        match project_id {
            Some(project_id) => {
                let _ = LocalStorage::set("active_project_id", project_id);
            }
            None => {
                let _ = LocalStorage::delete("active_project_id");
            }
        }
    }

    pub fn show_toast(
        &self,
        level: ToastLevel,
        title: impl Into<String>,
        message: impl Into<String>,
    ) {
        let id = TOAST_SEQUENCE.fetch_add(1, Ordering::Relaxed).to_string();
        self.toast.messages.update(|messages| {
            messages.push(ToastMessage {
                id,
                level,
                title: title.into(),
                message: message.into(),
                duration: Some(5000),
            });
        });
    }

    pub fn dismiss_toast(&self, id: &str) {
        self.toast
            .messages
            .update(|messages| messages.retain(|toast| toast.id != id));
    }
}
