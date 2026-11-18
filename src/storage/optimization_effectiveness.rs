use sqlx::PgPool;

const MIN_FEEDBACK_PER_WINDOW: i64 = 10;

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg(test)]
struct EffectivenessMetrics {
    predicted_improvement_pp: f64,
    actual_improvement_pp: f64,
    absolute_error_pp: f64,
    within_five_percentage_points: bool,
}

#[cfg(test)]
fn calculate_effectiveness(
    predicted_improvement_pp: f64,
    baseline_samples: i64,
    baseline_mismatches: i64,
    candidate_samples: i64,
    candidate_mismatches: i64,
) -> Option<EffectivenessMetrics> {
    if baseline_samples < MIN_FEEDBACK_PER_WINDOW
        || candidate_samples < MIN_FEEDBACK_PER_WINDOW
        || baseline_mismatches < 0
        || candidate_mismatches < 0
        || baseline_mismatches > baseline_samples
        || candidate_mismatches > candidate_samples
    {
        return None;
    }

    let baseline_mismatch_rate = baseline_mismatches as f64 / baseline_samples as f64 * 100.0;
    let candidate_mismatch_rate = candidate_mismatches as f64 / candidate_samples as f64 * 100.0;
    let actual_improvement_pp = baseline_mismatch_rate - candidate_mismatch_rate;
    let absolute_error_pp = (predicted_improvement_pp - actual_improvement_pp).abs();

    Some(EffectivenessMetrics {
        predicted_improvement_pp,
        actual_improvement_pp,
        absolute_error_pp,
        within_five_percentage_points: absolute_error_pp <= 5.0,
    })
}

pub async fn track_proposal_effectiveness(db: &PgPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        WITH RECURSIVE proposal_lineage AS (
            SELECT proposal.id AS proposal_id,
                   proposal.project_id,
                   proposal.source_version_id,
                   proposal.resulting_version_id,
                   proposal.resulting_version_id AS version_id
            FROM decision_optimization_proposals proposal
            WHERE proposal.status = 'confirmed'
              AND proposal.effectiveness_tracked_at IS NULL
              AND proposal.resulting_version_id IS NOT NULL

            UNION

            SELECT lineage.proposal_id,
                   lineage.project_id,
                   lineage.source_version_id,
                   lineage.resulting_version_id,
                   child.id AS version_id
            FROM proposal_lineage lineage
            JOIN decision_versions child
              ON child.base_version_id = lineage.version_id
             AND child.project_id = lineage.project_id
        ), eligible AS (
            SELECT proposal.id AS proposal_id,
                   proposal.project_id,
                   proposal.source_version_id,
                   proposal.resulting_version_id,
                   published.version_id AS published_version_id,
                   resulting.change_summary->>'candidateVersionId' AS candidate_version_id,
                   published.created_at AS published_at,
                   simulation.bias_rate_baseline::DOUBLE PRECISION
                       - simulation.bias_rate_simulated::DOUBLE PRECISION AS predicted_improvement_pp
            FROM decision_optimization_proposals proposal
            JOIN decision_versions resulting
              ON resulting.id = proposal.resulting_version_id
             AND resulting.project_id = proposal.project_id
            JOIN LATERAL (
                SELECT lineage.version_id, event.created_at
                FROM audit_events event
                JOIN proposal_lineage lineage
                  ON lineage.proposal_id = proposal.id
                 AND lineage.version_id = event.version_id
                JOIN decision_versions published_version
                  ON published_version.id = lineage.version_id
                 AND published_version.project_id = proposal.project_id
                 AND published_version.status IN ('published', 'retired')
                WHERE event.event_type = 'version.published'
                ORDER BY event.created_at, event.id
                LIMIT 1
            ) published ON TRUE
            JOIN LATERAL (
                SELECT bias_rate_baseline, bias_rate_simulated
                FROM optimization_simulations
                WHERE suggestion_id = proposal.id
                  AND version_id = NULLIF(resulting.change_summary->>'candidateVersionId', '')::BIGINT
                  AND passed
                ORDER BY created_at DESC, id DESC
                LIMIT 1
            ) simulation ON TRUE
            WHERE proposal.status = 'confirmed'
              AND proposal.effectiveness_tracked_at IS NULL
              AND resulting.change_summary ? 'candidateVersionId'
              AND published.created_at <= NOW() - INTERVAL '7 days'
        ), feedback AS (
            SELECT eligible.*,
                   COUNT(*) FILTER (
                       WHERE run.version_id = eligible.source_version_id
                         AND run.executed_at >= eligible.published_at - INTERVAL '7 days'
                         AND run.executed_at < eligible.published_at
                         AND run.status = 'succeeded'
                         AND run.outcome_assessment IN ('confirmed', 'mismatch')
                         AND run.outcome_reported_at IS NOT NULL
                   )::BIGINT AS baseline_7d_sample_count,
                   COUNT(*) FILTER (
                       WHERE run.version_id = eligible.source_version_id
                         AND run.executed_at >= eligible.published_at - INTERVAL '7 days'
                         AND run.executed_at < eligible.published_at
                         AND run.status = 'succeeded'
                         AND run.outcome_assessment = 'mismatch'
                         AND run.outcome_reported_at IS NOT NULL
                   )::BIGINT AS baseline_7d_mismatch_count,
                   COUNT(*) FILTER (
                       WHERE run.version_id = eligible.published_version_id
                         AND run.executed_at >= eligible.published_at
                         AND run.executed_at < eligible.published_at + INTERVAL '7 days'
                         AND run.status = 'succeeded'
                         AND run.outcome_assessment IN ('confirmed', 'mismatch')
                         AND run.outcome_reported_at IS NOT NULL
                   )::BIGINT AS candidate_7d_sample_count,
                   COUNT(*) FILTER (
                       WHERE run.version_id = eligible.published_version_id
                         AND run.executed_at >= eligible.published_at
                         AND run.executed_at < eligible.published_at + INTERVAL '7 days'
                         AND run.status = 'succeeded'
                         AND run.outcome_assessment = 'mismatch'
                         AND run.outcome_reported_at IS NOT NULL
                   )::BIGINT AS candidate_7d_mismatch_count,
                   COUNT(*) FILTER (
                       WHERE run.version_id = eligible.source_version_id
                         AND run.executed_at >= eligible.published_at - INTERVAL '30 days'
                         AND run.executed_at < eligible.published_at
                         AND run.status = 'succeeded'
                         AND run.outcome_assessment IN ('confirmed', 'mismatch')
                         AND run.outcome_reported_at IS NOT NULL
                   )::BIGINT AS baseline_sample_count,
                   COUNT(*) FILTER (
                       WHERE run.version_id = eligible.source_version_id
                         AND run.executed_at >= eligible.published_at - INTERVAL '30 days'
                         AND run.executed_at < eligible.published_at
                         AND run.status = 'succeeded'
                         AND run.outcome_assessment = 'mismatch'
                         AND run.outcome_reported_at IS NOT NULL
                   )::BIGINT AS baseline_mismatch_count,
                   COUNT(*) FILTER (
                       WHERE run.version_id = eligible.published_version_id
                         AND run.executed_at >= eligible.published_at
                         AND run.executed_at < eligible.published_at + INTERVAL '30 days'
                         AND run.status = 'succeeded'
                         AND run.outcome_assessment IN ('confirmed', 'mismatch')
                         AND run.outcome_reported_at IS NOT NULL
                   )::BIGINT AS candidate_sample_count,
                   COUNT(*) FILTER (
                       WHERE run.version_id = eligible.published_version_id
                         AND run.executed_at >= eligible.published_at
                         AND run.executed_at < eligible.published_at + INTERVAL '30 days'
                         AND run.status = 'succeeded'
                         AND run.outcome_assessment = 'mismatch'
                         AND run.outcome_reported_at IS NOT NULL
                   )::BIGINT AS candidate_mismatch_count
            FROM eligible
            LEFT JOIN decision_runs_partitioned run
              ON run.project_id = eligible.project_id
             AND run.version_id IN (eligible.source_version_id, eligible.published_version_id)
             AND run.executed_at >= eligible.published_at - INTERVAL '30 days'
             AND run.executed_at < eligible.published_at + INTERVAL '30 days'
            GROUP BY eligible.proposal_id, eligible.project_id, eligible.source_version_id,
                     eligible.resulting_version_id, eligible.published_version_id,
                     eligible.candidate_version_id,
                     eligible.published_at, eligible.predicted_improvement_pp
        ), calculated AS (
            SELECT feedback.*,
                   baseline_7d_mismatch_count * 100.0 / NULLIF(baseline_7d_sample_count, 0)
                     - candidate_7d_mismatch_count * 100.0 / NULLIF(candidate_7d_sample_count, 0)
                       AS actual_7d_improvement_pp,
                   baseline_mismatch_count * 100.0 / NULLIF(baseline_sample_count, 0)
                     - candidate_mismatch_count * 100.0 / NULLIF(candidate_sample_count, 0)
                       AS actual_improvement_pp
            FROM feedback
        )
        UPDATE decision_optimization_proposals proposal
        SET effectiveness_result_json = jsonb_build_object(
                'publishedAt', calculated.published_at,
                'candidateVersionId', calculated.candidate_version_id::BIGINT,
                'sevenDay', jsonb_build_object(
                    'status', CASE
                        WHEN calculated.baseline_7d_sample_count >= $1
                         AND calculated.candidate_7d_sample_count >= $1 THEN 'complete'
                        ELSE 'insufficient_samples'
                    END,
                    'predictedImprovementPp', calculated.predicted_improvement_pp,
                    'actualImprovementPp', calculated.actual_7d_improvement_pp,
                    'absoluteErrorPp', ABS(calculated.predicted_improvement_pp - calculated.actual_7d_improvement_pp),
                    'withinFivePercentagePoints',
                        ABS(calculated.predicted_improvement_pp - calculated.actual_7d_improvement_pp) <= 5.0,
                    'baselineSampleCount', calculated.baseline_7d_sample_count,
                    'baselineMismatchCount', calculated.baseline_7d_mismatch_count,
                    'candidateSampleCount', calculated.candidate_7d_sample_count,
                    'candidateMismatchCount', calculated.candidate_7d_mismatch_count
                ),
                'thirtyDay', jsonb_build_object(
                    'status', CASE
                        WHEN calculated.published_at > NOW() - INTERVAL '30 days' THEN 'pending'
                        WHEN calculated.baseline_sample_count >= $1
                         AND calculated.candidate_sample_count >= $1 THEN 'complete'
                        ELSE 'insufficient_samples'
                    END,
                    'predictedImprovementPp', calculated.predicted_improvement_pp,
                    'actualImprovementPp', calculated.actual_improvement_pp,
                    'absoluteErrorPp', ABS(calculated.predicted_improvement_pp - calculated.actual_improvement_pp),
                    'withinFivePercentagePoints',
                        ABS(calculated.predicted_improvement_pp - calculated.actual_improvement_pp) <= 5.0,
                    'baselineSampleCount', calculated.baseline_sample_count,
                    'baselineMismatchCount', calculated.baseline_mismatch_count,
                    'candidateSampleCount', calculated.candidate_sample_count,
                    'candidateMismatchCount', calculated.candidate_mismatch_count
                ),
                'predictedImprovementPp', calculated.predicted_improvement_pp,
                'actualImprovementPp', calculated.actual_improvement_pp,
                'absoluteErrorPp', ABS(calculated.predicted_improvement_pp - calculated.actual_improvement_pp),
                'withinFivePercentagePoints',
                    ABS(calculated.predicted_improvement_pp - calculated.actual_improvement_pp) <= 5.0,
                'baselineSampleCount', calculated.baseline_sample_count,
                'baselineMismatchCount', calculated.baseline_mismatch_count,
                'candidateSampleCount', calculated.candidate_sample_count,
                'candidateMismatchCount', calculated.candidate_mismatch_count,
                'trackedAt', CASE
                    WHEN calculated.published_at <= NOW() - INTERVAL '30 days'
                     AND calculated.baseline_sample_count >= $1
                     AND calculated.candidate_sample_count >= $1 THEN NOW()
                    ELSE NULL
                END
            ),
            effectiveness_tracked_at = CASE
                WHEN calculated.published_at <= NOW() - INTERVAL '30 days'
                 AND calculated.baseline_sample_count >= $1
                 AND calculated.candidate_sample_count >= $1 THEN NOW()
                ELSE NULL
            END
        FROM calculated
        WHERE proposal.id = calculated.proposal_id
          AND proposal.effectiveness_tracked_at IS NULL
        "#,
    )
    .bind(MIN_FEEDBACK_PER_WINDOW)
    .execute(db)
    .await?;
    Ok(result.rows_affected())
}

pub async fn track_proposal_effectiveness_daily(db: PgPool) {
    loop {
        match track_proposal_effectiveness(&db).await {
            Ok(count) if count > 0 => println!("已完成 {count} 条优化提案 30 天效果追踪"),
            Ok(_) => {}
            Err(error) => eprintln!("优化提案效果追踪失败：{error}"),
        }
        tokio::time::sleep(std::time::Duration::from_secs(24 * 60 * 60)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effectiveness_requires_ten_conclusive_feedback_samples_per_window() {
        assert!(calculate_effectiveness(10.0, 9, 1, 10, 0).is_none());
        assert!(calculate_effectiveness(10.0, 10, 1, 9, 0).is_none());
        assert!(calculate_effectiveness(10.0, 10, 11, 10, 0).is_none());
    }

    #[test]
    fn effectiveness_compares_predicted_and_actual_mismatch_reduction() {
        let metrics = calculate_effectiveness(10.0, 20, 8, 20, 6).unwrap();
        assert_eq!(metrics.actual_improvement_pp, 10.0);
        assert_eq!(metrics.absolute_error_pp, 0.0);
        assert!(metrics.within_five_percentage_points);

        let metrics = calculate_effectiveness(10.0, 20, 8, 20, 8).unwrap();
        assert_eq!(metrics.actual_improvement_pp, 0.0);
        assert_eq!(metrics.absolute_error_pp, 10.0);
        assert!(!metrics.within_five_percentage_points);
    }
}
