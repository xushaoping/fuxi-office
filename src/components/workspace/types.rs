use leptos::prelude::*;

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

impl GenerationMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Create => "创建模型",
            Self::Modify => "修改模型",
        }
    }

    pub fn button_label(self) -> &'static str {
        match self {
            Self::Create => "创建新模型",
            Self::Modify => "修改已有模型",
        }
    }
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
pub enum MobileModelFocus {
    Conversation,
    Inspector,
}

impl MobileModelFocus {
    pub fn class(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
            Self::Inspector => "inspector",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceView {
    Knowledge,
    Models,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyStatus {
    Idle,
    Copied,
    Failed,
}

pub const MAX_DMN_COMPILE_REPAIRS: i32 = 2;

pub fn is_repairable_model_change_error(message: &str) -> bool {
    message.starts_with("[DMN_COMPILE]")
}

pub fn audit_event_kind(event_type: &str) -> &'static str {
    match event_type {
        "test_cases.generated" | "test_case.approved" | "test_case.approval_revoked" => "test-case",
        "version.verified" => "verification",
        "version.published" => "published",
        "version.retired" => "retired",
        _ => "general",
    }
}

pub fn audit_event_type_label(event_type: &str) -> &'static str {
    match event_type {
        "project.created" => "项目创建",
        "model.reused" => "模型复用",
        "version.created" => "版本创建",
        "version.rollback_created" => "回滚草稿",
        "version.verified" => "自动验收",
        "version.reviewed" => "独立复核",
        "version.published" => "版本发布",
        "version.retired" => "版本退役",
        "test_cases.generated" => "案例生成",
        "test_case.approved" => "案例批准",
        "test_case.approval_revoked" => "撤销批准",
        "decision_service.invoked" => "服务调用",
        "project.member_added" => "成员添加",
        "project.member_removed" => "成员移除",
        _ => "其他事件",
    }
}
