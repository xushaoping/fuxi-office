use leptos::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Theme {
    Light,
    Dark,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::Dark
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum WorkspaceView {
    Discovery,
    Model,
    Validation,
    Services,
    Analytics,
    Versions,
    Collaboration,
    Resources,
}

impl Default for WorkspaceView {
    fn default() -> Self {
        WorkspaceView::Discovery
    }
}

#[derive(Clone, Debug)]
pub struct UiState {
    pub theme: RwSignal<Theme>,
    pub sidebar_open: RwSignal<bool>,
    pub active_workspace: RwSignal<WorkspaceView>,
}

impl UiState {
    pub fn new() -> Self {
        // Try to load theme from localStorage
        let theme = Theme::default();

        Self {
            theme: RwSignal::new(theme),
            sidebar_open: RwSignal::new(true),
            active_workspace: RwSignal::new(WorkspaceView::Discovery),
        }
    }

    pub fn toggle_theme(&self) {
        self.theme.update(|theme| {
            *theme = match *theme {
                Theme::Light => Theme::Dark,
                Theme::Dark => Theme::Light,
            };
        });
    }

    pub fn toggle_sidebar(&self) {
        self.sidebar_open.update(|open| *open = !*open);
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}
