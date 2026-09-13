use gloo_storage::{LocalStorage, Storage};
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

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

#[derive(Clone, Debug)]
pub struct AuthState {
    pub token: RwSignal<Option<String>>,
    pub user: RwSignal<Option<String>>,
}

#[derive(Clone, Debug)]
pub struct UiState {
    pub theme: RwSignal<Theme>,
    pub sidebar_open: RwSignal<bool>,
    pub active_workspace: RwSignal<WorkspaceView>,
    /// Set by the modeling workspace while local input or an unconfirmed
    /// model change differs from the server snapshot.
    pub workspace_dirty: RwSignal<bool>,
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

#[derive(Clone, Debug)]
pub struct ToastState {
    pub messages: RwSignal<Vec<ToastMessage>>,
}

#[derive(Clone, Debug)]
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
