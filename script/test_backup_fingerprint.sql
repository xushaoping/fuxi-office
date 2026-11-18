-- Test-fixture fingerprint only: string_agg intentionally targets small isolated
-- fixtures. Production reconciliation must use bounded, business-specific scans.
-- Skip physical child partitions (the parent already includes their rows).
SELECT format(
    'SELECT %L, count(*), md5(COALESCE(string_agg(row_data, '''' ORDER BY row_data), '''')) FROM (SELECT row_to_json(t)::text AS row_data FROM public.%I t) rows',
    c.relname, c.relname)
FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p', 'm')
  AND NOT c.relispartition
ORDER BY c.relname
\gexec
