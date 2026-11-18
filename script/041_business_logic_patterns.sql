BEGIN;

CREATE TABLE IF NOT EXISTS business_logic_patterns (
    id BIGSERIAL PRIMARY KEY,
    pattern_type TEXT NOT NULL UNIQUE
        CHECK (pattern_type IN ('tree', 'matrix', 'workflow', 'threshold')),
    description TEXT NOT NULL,
    keywords TEXT[] NOT NULL DEFAULT '{}',
    typical_structures JSONB NOT NULL DEFAULT '{}'::JSONB,
    sample_clarifications JSONB NOT NULL DEFAULT '[]'::JSONB,
    success_rate NUMERIC(5, 2)
        CHECK (success_rate IS NULL OR (success_rate >= 0 AND success_rate <= 100)),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO business_logic_patterns (
    pattern_type, description, keywords, typical_structures, sample_clarifications
) VALUES
    (
        'tree', '多条件分支、分级或互斥规则',
        ARRAY['如果', '若', '否则', '条件', '分级'],
        '{"shape":"conditions -> branches -> outcome"}'::JSONB,
        '["多个条件同时满足时如何处理？", "未匹配任何条件时的默认结果是什么？"]'::JSONB
    ),
    (
        'matrix', '多个维度、指标或权重共同计算评分',
        ARRAY['评分', '打分', '权重', '维度', '综合得分'],
        '{"shape":"dimensions -> weights -> score -> category"}'::JSONB,
        '["各维度的权重或评分范围是什么？", "总分边界如何映射到结果？"]'::JSONB
    ),
    (
        'workflow', '按顺序执行的校验、审批或处理步骤',
        ARRAY['流程', '步骤', '首先', '然后', '审批'],
        '{"shape":"ordered steps -> gates -> terminal outcome"}'::JSONB,
        '["步骤失败后是否继续后续流程？", "哪些步骤允许并行处理？"]'::JSONB
    ),
    (
        'threshold', '基于边界、区间或门槛判定结果',
        ARRAY['阈值', '超过', '低于', '至少', '区间'],
        '{"shape":"variable + boundaries -> outcome"}'::JSONB,
        '["边界值属于哪一侧？", "阈值之间的空档如何处理？"]'::JSONB
    )
ON CONFLICT (pattern_type) DO NOTHING;

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE
    ON TABLE business_logic_patterns TO :"app_user";
GRANT USAGE, SELECT, UPDATE
    ON SEQUENCE business_logic_patterns_id_seq TO :"app_user";
\endif

COMMIT;
