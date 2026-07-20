-- HLR-07/10: a signed event and its opaque current projection advance in the
-- same database transaction. Operational gauges contain no project, user, or
-- encrypted-domain labels.

CREATE TABLE sync_current_projections (
    project_id uuid NOT NULL,
    resource_node_id uuid NOT NULL,
    aggregate_version bigint NOT NULL CHECK (aggregate_version > 0),
    mutation_kind text NOT NULL CHECK (mutation_kind IN ('upsert', 'tombstone')),
    key_epoch integer NOT NULL CHECK (key_epoch > 0),
    encrypted_payload bytea NOT NULL CHECK (octet_length(encrypted_payload) > 0),
    event_id uuid NOT NULL,
    event_hash bytea NOT NULL CHECK (octet_length(event_hash) = 32),
    updated_at timestamptz NOT NULL,
    PRIMARY KEY (project_id, resource_node_id),
    CONSTRAINT sync_current_projections_resource_fk
        FOREIGN KEY (project_id, resource_node_id)
        REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT
);

CREATE FUNCTION sprout_private.advance_sync_projection()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO sync_current_projections (
        project_id, resource_node_id, aggregate_version, mutation_kind,
        key_epoch, encrypted_payload, event_id, event_hash, updated_at
    )
    VALUES (
        NEW.project_id, NEW.resource_node_id, NEW.aggregate_version,
        NEW.mutation_kind, NEW.key_epoch, NEW.encrypted_payload,
        NEW.id, NEW.event_hash, NEW.received_at
    )
    ON CONFLICT (project_id, resource_node_id) DO UPDATE
    SET aggregate_version = EXCLUDED.aggregate_version,
        mutation_kind = EXCLUDED.mutation_kind,
        key_epoch = EXCLUDED.key_epoch,
        encrypted_payload = EXCLUDED.encrypted_payload,
        event_id = EXCLUDED.event_id,
        event_hash = EXCLUDED.event_hash,
        updated_at = EXCLUDED.updated_at;
    RETURN NEW;
END;
$$;

CREATE TRIGGER sync_events_advance_projection
AFTER INSERT ON sync_events
FOR EACH ROW EXECUTE FUNCTION sprout_private.advance_sync_projection();

INSERT INTO sync_current_projections (
    project_id, resource_node_id, aggregate_version, mutation_kind,
    key_epoch, encrypted_payload, event_id, event_hash, updated_at
)
SELECT DISTINCT ON (project_id, resource_node_id)
    project_id,
    resource_node_id,
    aggregate_version,
    mutation_kind,
    key_epoch,
    encrypted_payload,
    id,
    event_hash,
    received_at
FROM sync_events
ORDER BY project_id, resource_node_id, aggregate_version DESC;

ALTER TABLE sync_current_projections ENABLE ROW LEVEL SECURITY;
ALTER TABLE sync_current_projections FORCE ROW LEVEL SECURITY;
CREATE POLICY sync_current_projections_resource_read ON sync_current_projections
    FOR SELECT
    USING (
        sprout_private.can_access_resource(project_id, resource_node_id, 'read')
    );
CREATE POLICY sync_current_projections_resource_insert ON sync_current_projections
    FOR INSERT
    WITH CHECK (
        sprout_private.can_access_resource(project_id, resource_node_id, 'write')
    );
CREATE POLICY sync_current_projections_resource_update ON sync_current_projections
    FOR UPDATE
    USING (
        sprout_private.can_access_resource(project_id, resource_node_id, 'write')
    )
    WITH CHECK (
        sprout_private.can_access_resource(project_id, resource_node_id, 'write')
    );
CREATE POLICY sync_current_projections_retention_delete ON sync_current_projections
    FOR DELETE
    USING (sprout_private.retention_purge_row_allowed(to_jsonb(sync_current_projections)));
CREATE TRIGGER sync_current_projections_retention_delete
BEFORE DELETE ON sync_current_projections
FOR EACH ROW EXECUTE FUNCTION sprout_private.retention_only_delete();

CREATE FUNCTION sprout_private.delete_sync_projection_with_aggregate()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    DELETE FROM sync_current_projections
    WHERE project_id = OLD.project_id
      AND resource_node_id = OLD.resource_node_id;
    RETURN OLD;
END;
$$;

CREATE TRIGGER sync_aggregates_delete_projection
BEFORE DELETE ON sync_aggregates
FOR EACH ROW EXECUTE FUNCTION sprout_private.delete_sync_projection_with_aggregate();

CREATE TABLE operational_metrics (
    name text PRIMARY KEY CHECK (name IN ('worker_lag_seconds')),
    value double precision NOT NULL CHECK (
        value >= 0 AND value < 'Infinity'::double precision
    ),
    updated_at timestamptz NOT NULL DEFAULT now()
);

INSERT INTO operational_metrics (name, value)
VALUES ('worker_lag_seconds', 0);

REVOKE ALL ON FUNCTION sprout_private.advance_sync_projection() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.delete_sync_projection_with_aggregate() FROM PUBLIC;
