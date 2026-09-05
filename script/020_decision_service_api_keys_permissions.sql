-- Repair grants for the API-key table on databases initialized before the
-- table was added to the consolidated init.sql grant list.
-- Run while connected to the application database, for example:
--   psql -U <管理员> -d fuxi_office -v app_user=fuxi -f script/020_decision_service_api_keys_permissions.sql

\if :{?app_user}
GRANT USAGE ON SCHEMA public TO :"app_user";
GRANT SELECT, INSERT, UPDATE, DELETE
    ON TABLE decision_service_api_keys TO :"app_user";
GRANT USAGE, SELECT, UPDATE
    ON SEQUENCE decision_service_api_keys_id_seq TO :"app_user";
\endif
