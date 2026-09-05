#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStatus {
    Idle,
    Running,
    Succeeded,
    Failed,
    Aborted,
}

impl RunStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "准备就绪",
            Self::Running => "智能体运行中",
            Self::Succeeded => "已完成",
            Self::Failed => "运行失败",
            Self::Aborted => "已停止",
        }
    }

    pub fn class(self) -> &'static str {
        match self {
            Self::Idle => "status-idle",
            Self::Running => "status-running",
            Self::Succeeded => "status-success",
            Self::Failed => "status-error",
            Self::Aborted => "status-muted",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenerationMode {
    Create,
    Modify,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceView {
    Knowledge,
    Models,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

impl Theme {
    pub fn class(self) -> &'static str {
        match self {
            Self::Dark => "theme-dark",
            Self::Light => "theme-light",
        }
    }

    pub fn toggle_label(self) -> &'static str {
        match self {
            Self::Dark => "切换到浅色主题",
            Self::Light => "切换到深色主题",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyStatus {
    Idle,
    Copied,
    Failed,
}

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub role: &'static str,
    pub content: String,
}
