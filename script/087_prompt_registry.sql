BEGIN;

CREATE TABLE IF NOT EXISTS prompt_assets (
    id BIGSERIAL PRIMARY KEY,
    prompt_key TEXT NOT NULL CHECK (prompt_key IN ('requirement_analysis', 'decision_planning', 'dsl_generation', 'change_intent')),
    version TEXT NOT NULL CHECK (version ~ '^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$'),
    content TEXT NOT NULL CHECK (length(content) BETWEEN 1 AND 100000),
    content_digest CHAR(64) NOT NULL CHECK (content_digest ~ '^[0-9a-f]{64}$'),
    status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'evaluated', 'active', 'retired')),
    evaluation_json JSONB,
    created_by TEXT NOT NULL REFERENCES users(username) ON DELETE RESTRICT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    evaluated_at TIMESTAMPTZ,
    activated_at TIMESTAMPTZ,
    retired_at TIMESTAMPTZ,
    UNIQUE (prompt_key, version)
);

CREATE UNIQUE INDEX IF NOT EXISTS prompt_assets_one_active_idx
    ON prompt_assets (prompt_key) WHERE status = 'active';
CREATE INDEX IF NOT EXISTS prompt_assets_history_idx
    ON prompt_assets (prompt_key, created_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS prompt_asset_events (
    id BIGSERIAL PRIMARY KEY,
    prompt_asset_id BIGINT NOT NULL REFERENCES prompt_assets(id) ON DELETE RESTRICT,
    event_type TEXT NOT NULL CHECK (event_type IN ('draft_created', 'evaluated', 'activated', 'rollback')),
    actor_username TEXT NOT NULL REFERENCES users(username) ON DELETE RESTRICT,
    details_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS prompt_asset_events_history_idx
    ON prompt_asset_events (prompt_asset_id, created_at DESC, id DESC);

CREATE OR REPLACE FUNCTION prevent_prompt_asset_content_mutation()
RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP = 'DELETE'
       OR NEW.prompt_key IS DISTINCT FROM OLD.prompt_key
       OR NEW.version IS DISTINCT FROM OLD.version
       OR NEW.content IS DISTINCT FROM OLD.content
       OR NEW.content_digest IS DISTINCT FROM OLD.content_digest
       OR NEW.created_by IS DISTINCT FROM OLD.created_by
       OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN
        RAISE EXCEPTION 'prompt asset content versions are immutable';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION prevent_prompt_asset_event_mutation()
RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'prompt asset events are append-only';
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS prompt_assets_immutable ON prompt_assets;
CREATE TRIGGER prompt_assets_immutable
    BEFORE UPDATE OR DELETE ON prompt_assets
    FOR EACH ROW EXECUTE FUNCTION prevent_prompt_asset_content_mutation();
DROP TRIGGER IF EXISTS prompt_asset_events_append_only ON prompt_asset_events;
CREATE TRIGGER prompt_asset_events_append_only
    BEFORE UPDATE OR DELETE ON prompt_asset_events
    FOR EACH ROW EXECUTE FUNCTION prevent_prompt_asset_event_mutation();

COMMENT ON TABLE prompt_assets IS
    'Immutable prompt content versions with guarded draft, evaluation, activation and retirement states.';
COMMENT ON TABLE prompt_asset_events IS
    'Append-only audit history for prompt asset lifecycle operations.';

GRANT SELECT, INSERT, UPDATE ON TABLE prompt_assets TO :"app_user";
GRANT SELECT, INSERT ON TABLE prompt_asset_events TO :"app_user";
GRANT USAGE, SELECT ON SEQUENCE prompt_assets_id_seq, prompt_asset_events_id_seq TO :"app_user";

COMMIT;
