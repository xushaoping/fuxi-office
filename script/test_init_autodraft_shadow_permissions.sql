DO $$
BEGIN
    IF NOT (
        has_table_privilege('fuxi', 'public.optimization_autodraft_shadow_evaluations', 'SELECT')
        AND has_table_privilege('fuxi', 'public.optimization_autodraft_shadow_evaluations', 'INSERT')
        AND has_table_privilege('fuxi', 'public.optimization_autodraft_shadow_evaluations', 'UPDATE')
        AND has_table_privilege('fuxi', 'public.optimization_autodraft_shadow_evaluations', 'DELETE')
    ) THEN
        RAISE EXCEPTION 'fresh initialization did not grant fuxi CRUD privileges on shadow evaluations';
    END IF;

    IF NOT (
        has_sequence_privilege('fuxi', 'public.optimization_autodraft_shadow_evaluations_id_seq', 'USAGE')
        AND has_sequence_privilege('fuxi', 'public.optimization_autodraft_shadow_evaluations_id_seq', 'SELECT')
        AND NOT has_sequence_privilege('fuxi', 'public.optimization_autodraft_shadow_evaluations_id_seq', 'UPDATE')
    ) THEN
        RAISE EXCEPTION 'fresh initialization must grant sequence usage/read while denying setval on shadow evaluations';
    END IF;
END;
$$;
