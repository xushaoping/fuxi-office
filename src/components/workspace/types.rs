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

impl WorkspaceView {
    pub const ALL: [Self; 8] = [
        Self::Discovery,
        Self::Model,
        Self::Validation,
        Self::Services,
        Self::Analytics,
        Self::Versions,
        Self::Collaboration,
        Self::Resources,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Discovery => "discovery",
            Self::Model => "model",
            Self::Validation => "validation",
            Self::Services => "services",
            Self::Analytics => "analytics",
            Self::Versions => "versions",
            Self::Collaboration => "collaboration",
            Self::Resources => "resources",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Discovery => "需求与方案",
            Self::Model => "决策建模",
            Self::Validation => "案例验收",
            Self::Services => "决策服务",
            Self::Analytics => "运行分析",
            Self::Versions => "版本管理",
            Self::Collaboration => "项目协作",
            Self::Resources => "资源中心",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Discovery => "从业务目标出发，与智能体一起梳理规则，形成清晰、可验证的决策方案。",
            Self::Model => "通过对话迭代决策模型，审查每一次变更，让业务规则保持清晰可追溯。",
            Self::Validation => "用业务案例检验模型输出，完成案例批准与版本验收。",
            Self::Services => "管理已发布的决策服务、调用凭证与执行记录。",
            Self::Analytics => "洞察决策执行表现，发现规则风险与优化机会。",
            Self::Versions => "查看模型演进记录，完成版本审查、发布与回滚。",
            Self::Collaboration => "明确建模、复核与发布职责，与团队一起交付可信决策。",
            Self::Resources => "复用知识来源与行业模型，为新的决策场景提供起点。",
        }
    }

    pub fn unlock_hint(self) -> &'static str {
        match self {
            Self::Model => "生成模型后可进入决策建模",
            Self::Validation => "保存模型版本后可进行案例验收",
            Self::Services | Self::Analytics => "发布模型版本后可查看",
            Self::Versions => "保存首个版本后可查看版本记录",
            Self::Collaboration => "请先选择项目",
            _ => self.description(),
        }
    }
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
