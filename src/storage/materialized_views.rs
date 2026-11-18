use sqlx::PgPool;

pub async fn refresh_daily_run_stats(db: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT refresh_decision_run_daily_stats()")
        .execute(db)
        .await?;
    Ok(())
}

pub async fn refresh_daily_run_stats_daily(db: PgPool) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(24 * 60 * 60));
    interval.tick().await;
    loop {
        interval.tick().await;
        if let Err(error) = refresh_daily_run_stats(&db).await {
            eprintln!("刷新决策运行每日统计失败：{error}");
        }
    }
}
