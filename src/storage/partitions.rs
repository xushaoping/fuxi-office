use sqlx::PgPool;

pub async fn maintain_decision_run_partitions(db: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT maintain_decision_run_partitions(13)")
        .execute(db)
        .await?;
    Ok(())
}

pub async fn maintain_decision_run_partitions_daily(db: PgPool) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(24 * 60 * 60));
    loop {
        interval.tick().await;
        if let Err(error) = maintain_decision_run_partitions(&db).await {
            eprintln!("自动维护决策运行分区失败：{error}");
        }
    }
}
