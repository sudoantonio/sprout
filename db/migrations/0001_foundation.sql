-- Sprout PostgreSQL foundation.
-- Every timestamp represents an instant; clients and operators should render it in UTC.

CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE SCHEMA IF NOT EXISTS sprout_private;

CREATE OR REPLACE FUNCTION sprout_private.current_identity_id()
RETURNS uuid
LANGUAGE sql
STABLE
PARALLEL SAFE
AS $$
    SELECT NULLIF(current_setting('app.identity_id', true), '')::uuid
$$;

CREATE OR REPLACE FUNCTION sprout_private.current_device_id()
RETURNS uuid
LANGUAGE sql
STABLE
PARALLEL SAFE
AS $$
    SELECT NULLIF(current_setting('app.device_id', true), '')::uuid
$$;

CREATE OR REPLACE FUNCTION sprout_private.touch_updated_at()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    NEW.updated_at := clock_timestamp();
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION sprout_private.reject_historical_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION '% is append-only', TG_TABLE_NAME
        USING ERRCODE = '55000';
END;
$$;

COMMENT ON SCHEMA sprout_private IS
    'Security-definer and session-context helpers; not an application data schema.';
