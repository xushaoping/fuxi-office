use sqlx::PgPool;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectPermission {
    View,
    Model,
    ExecuteAcceptance,
    Review,
    Approve,
    Manage,
    ManageService,
}

impl ProjectPermission {
    fn allows(self, role: &str) -> bool {
        let role = role.trim().to_ascii_lowercase();
        matches!(
            (self, role.as_str()),
            (Self::View, "modeler" | "reviewer" | "approver" | "viewer")
                | (Self::Model, "modeler")
                | (Self::ExecuteAcceptance, "modeler" | "reviewer")
                | (Self::Review, "reviewer")
                | (Self::Approve, "approver")
                | (Self::ManageService, "approver")
        )
    }
}

/// Project roles are intentionally resolved from PostgreSQL for every request,
/// so a membership change takes effect immediately instead of waiting for JWT
/// expiry. The system admin is a super administrator; other owners keep
/// management, modelling, and acceptance-execution rights, while approvers may
/// additionally manage scoped external-service credentials.
pub async fn has_permission(
    db: &PgPool,
    project_id: i64,
    username: &str,
    permission: ProjectPermission,
) -> Result<bool, sqlx::Error> {
    let is_admin = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM users WHERE username = $1 AND role = 'admin')",
    )
    .bind(username)
    .fetch_one(db)
    .await?;
    if is_admin {
        return Ok(true);
    }
    let owner = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM decision_projects WHERE id = $1 AND owner_username = $2)",
    )
    .bind(project_id)
    .bind(username)
    .fetch_one(db)
    .await?;
    if owner
        && matches!(
            permission,
            ProjectPermission::View
                | ProjectPermission::Model
                | ProjectPermission::ExecuteAcceptance
                | ProjectPermission::Manage
                | ProjectPermission::ManageService
        )
    {
        return Ok(true);
    }
    let roles = sqlx::query_scalar::<_, String>(
        "SELECT role FROM project_members WHERE project_id = $1 AND username = $2",
    )
    .bind(project_id)
    .bind(username)
    .fetch_all(db)
    .await?;
    // Ownership does not implicitly grant delegated review or approval. If a
    // legacy project explicitly assigned one of those roles to its owner,
    // honor that assignment just like any other project member role.
    Ok(roles.iter().any(|role| permission.allows(role)))
}

#[cfg(test)]
mod tests {
    use super::ProjectPermission;

    #[test]
    fn delegated_roles_are_separated_by_responsibility() {
        assert!(ProjectPermission::Model.allows("modeler"));
        assert!(!ProjectPermission::Model.allows("reviewer"));
        assert!(!ProjectPermission::Model.allows("approver"));
        assert!(ProjectPermission::ExecuteAcceptance.allows("modeler"));
        assert!(ProjectPermission::ExecuteAcceptance.allows("reviewer"));
        assert!(!ProjectPermission::ExecuteAcceptance.allows("viewer"));
        assert!(!ProjectPermission::ExecuteAcceptance.allows("approver"));
        assert!(!ProjectPermission::Review.allows("modeler"));
        assert!(ProjectPermission::Review.allows("reviewer"));
        assert!(!ProjectPermission::Approve.allows("reviewer"));
        assert!(ProjectPermission::Approve.allows("approver"));
        assert!(ProjectPermission::ManageService.allows("approver"));
        assert!(!ProjectPermission::ManageService.allows("reviewer"));
        assert!(ProjectPermission::View.allows("viewer"));
        assert!(!ProjectPermission::Model.allows("viewer"));
        assert!(ProjectPermission::Approve.allows(" APPROVER "));
    }
}
