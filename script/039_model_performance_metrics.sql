BEGIN;

CREATE TABLE IF NOT EXISTS model_performance_metrics (
    id BIGSERIAL PRIMARY KEY,
    model_name TEXT NOT NULL,
    task_type TEXT NOT NULL,
    sample_count BIGINT NOT NULL DEFAULT 0 CHECK (sample_count >= 0),
    success_count BIGINT NOT NULL DEFAULT 0 CHECK (success_count >= 0),
    success_rate NUMERIC(5, 2) NOT NULL DEFAULT 0,
    avg_latency_ms INTEGER NOT NULL DEFAULT 0 CHECK (avg_latency_ms >= 0),
    estimated_cost_usd NUMERIC(12, 6),
    last_updated TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (model_name, task_type),
    CHECK (success_count <= sample_count),
    CHECK (success_rate >= 0 AND success_rate <= 100)
);

CREATE OR REPLACE FUNCTION record_model_performance_metric()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    normalized_task TEXT;
    run_latency_ms INTEGER;
    run_succeeded BIGINT;
BEGIN
    IF OLD.status <> 'running'
        OR NEW.status NOT IN ('succeeded', 'failed')
        OR NEW.model IS NULL
        OR NEW.finished_at IS NULL THEN
        RETURN NEW;
    END IF;

    normalized_task := CASE NEW.stage
        WHEN 'requirement_analysis' THEN 'analyze'
        WHEN 'decision_planning' THEN 'plan'
        WHEN 'dsl_generation' THEN 'dsl'
        WHEN 'change_intent' THEN 'change_intent'
        ELSE NEW.stage
    END;
    run_latency_ms := LEAST(
        2147483647,
        GREATEST(0, (EXTRACT(EPOCH FROM (NEW.finished_at - OLD.started_at)) * 1000)::BIGINT)
    )::INTEGER;
    run_succeeded := CASE WHEN NEW.status = 'succeeded' THEN 1 ELSE 0 END;

    INSERT INTO model_performance_metrics (
        model_name, task_type, sample_count, success_count, success_rate,
        avg_latency_ms, last_updated
    ) VALUES (
        NEW.model, normalized_task, 1, run_succeeded, run_succeeded * 100,
        run_latency_ms, NEW.finished_at
    )
    ON CONFLICT (model_name, task_type) DO UPDATE SET
        sample_count = model_performance_metrics.sample_count + 1,
        success_count = model_performance_metrics.success_count + EXCLUDED.success_count,
        success_rate = ROUND(
            (model_performance_metrics.success_count + EXCLUDED.success_count)::NUMERIC
            * 100 / (model_performance_metrics.sample_count + 1),
            2
        ),
        avg_latency_ms = (
            (model_performance_metrics.avg_latency_ms::BIGINT
                * model_performance_metrics.sample_count + EXCLUDED.avg_latency_ms)
            / (model_performance_metrics.sample_count + 1)
        )::INTEGER,
        last_updated = EXCLUDED.last_updated;

    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS agent_runs_record_model_performance ON agent_runs;
CREATE TRIGGER agent_runs_record_model_performance
    AFTER UPDATE OF status ON agent_runs
    FOR EACH ROW
    WHEN (OLD.status = 'running' AND NEW.status IN ('succeeded', 'failed'))
    EXECUTE FUNCTION record_model_performance_metric();

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE
    ON TABLE model_performance_metrics TO :"app_user";
GRANT USAGE, SELECT, UPDATE
    ON SEQUENCE model_performance_metrics_id_seq TO :"app_user";
\endif

COMMIT;
