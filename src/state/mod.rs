//! Shared reactive state for the CSR application.

mod app_state;

pub use app_state::{
    AppState, AuthState, Theme, ToastLevel, ToastMessage, ToastState, UiState,
    WorkspaceDraftSnapshot, WorkspaceView,
};
