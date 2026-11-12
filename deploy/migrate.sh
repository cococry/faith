#!/bin/sh
set -eu
umask 077
export PGPASSWORD="$(cat /run/secrets/db_admin_password)"
export FAITH_APP_PASSWORD="$(cat /run/secrets/db_app_password)"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
{
  printf '%s\n' '\set ON_ERROR_STOP on' 'BEGIN;' 'SELECT pg_advisory_xact_lock(170001);'
  printf '%s\n' 'CREATE TABLE IF NOT EXISTS faith_schema_migrations(version text PRIMARY KEY, checksum text NOT NULL);'
  for migration in /app/migrations/[0-9]*.sql; do
    version=$(basename "$migration")
    checksum=$(sha256sum "$migration" | cut -d ' ' -f 1)
    sed '/^BEGIN;$/d; /^COMMIT;$/d' "$migration" > "$work/$version"
    printf "SELECT EXISTS(SELECT 1 FROM faith_schema_migrations WHERE version='%s') AS applied \\gset\n" "$version"
    printf '%s\n' '\if :applied'
    printf "SELECT checksum='%s' AS unchanged FROM faith_schema_migrations WHERE version='%s' \\gset\n" "$checksum" "$version"
    printf '%s\n' '\if :unchanged' '\else' "DO \$\$ BEGIN RAISE EXCEPTION 'Previously applied migration changed'; END \$\$;" '\endif' '\else'
    printf '\\i %s\n' "$work/$version"
    printf "INSERT INTO faith_schema_migrations VALUES('%s','%s');\n" "$version" "$checksum"
    printf '%s\n' '\endif'
  done
  cat <<'SQL'
\getenv app_password FAITH_APP_PASSWORD
SELECT 'CREATE ROLE faith_app LOGIN' WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='faith_app') \gexec
SELECT format('ALTER ROLE faith_app PASSWORD %L', :'app_password') \gexec
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
GRANT USAGE ON SCHEMA public TO faith_app;
GRANT SELECT,INSERT,UPDATE,DELETE ON users,devices,device_event_inboxes,device_events,event_storage_usage TO faith_app;
GRANT EXECUTE ON FUNCTION expire_device_events() TO faith_app;
COMMIT;
SQL
} > "$work/upgrade.sql"
psql -X -f "$work/upgrade.sql"
