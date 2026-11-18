BEGIN;

CREATE OR REPLACE FUNCTION record_model_performance_metric()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    normalized_task TEXT;
    normalized_model TEXT;
    run_latency_ms INTEGER;
    run_succeeded BIGINT;
BEGIN
    IF OLD.status <> 'running'
        OR NEW.status NOT IN ('succeeded', 'failed')
        OR NEW.model IS NULL
        OR NEW.finished_at IS NULL THEN
        RETURN NEW;
    END IF;

    IF NEW.usage_json ? 'response_models' THEN
        IF jsonb_typeof(NEW.usage_json->'response_models') IS DISTINCT FROM 'array' THEN
            RETURN NEW;
        END IF;
        IF jsonb_array_length(NEW.usage_json->'response_models') <> 1 THEN
            RETURN NEW;
        END IF;
        normalized_model := NULLIF(BTRIM(NEW.usage_json->'response_models'->>0), '');
        IF normalized_model IS NULL THEN
            RETURN NEW;
        END IF;
    ELSE
        -- Keep compatibility with runs started by a pre-upgrade application process.
        normalized_model := NEW.model;
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
        normalized_model, normalized_task, 1, run_succeeded, run_succeeded * 100,
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

COMMIT;
