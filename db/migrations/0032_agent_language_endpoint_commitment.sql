-- Provider-neutral endpoint request witness for client-owned inference.
-- The commitment is produced and dual-signed by the authorized edge device.
-- It contains no provider, model, credential, endpoint, network topology or
-- plaintext. Legacy 0031 observations remain NULL and are not retroactively
-- promoted to client-provider exactness.

ALTER TABLE agent_model_attempt_dispatches
    ADD COLUMN runtime_kind text NOT NULL DEFAULT 'legacy_0031'
        CHECK (runtime_kind IN ('legacy_0031', 'client_provider_v1')),
    ADD COLUMN execution_profile_commitment bytea CHECK (
        execution_profile_commitment IS NULL
        OR octet_length(execution_profile_commitment) = 32
    ),
    ADD CONSTRAINT agent_model_dispatch_runtime_profile_shape CHECK (
        (runtime_kind = 'legacy_0031' AND execution_profile_commitment IS NULL)
        OR
        (runtime_kind = 'client_provider_v1' AND execution_profile_commitment IS NOT NULL)
    );

ALTER TABLE agent_invocations
    ADD COLUMN required_runtime_kind text NOT NULL DEFAULT 'legacy_0031'
        CHECK (required_runtime_kind IN ('legacy_0031', 'client_provider_v1'));

ALTER TABLE agent_model_attempt_observations
    ADD COLUMN runtime_kind text NOT NULL DEFAULT 'legacy_0031'
        CHECK (runtime_kind IN ('legacy_0031', 'client_provider_v1')),
    ADD COLUMN endpoint_request_exact boolean NOT NULL DEFAULT false,
    ADD COLUMN endpoint_request_commitment bytea
        CHECK (
            endpoint_request_commitment IS NULL
            OR octet_length(endpoint_request_commitment) = 32
        ),
    ADD COLUMN execution_profile_commitment bytea CHECK (
        execution_profile_commitment IS NULL
        OR octet_length(execution_profile_commitment) = 32
    ),
    ADD CONSTRAINT agent_model_observation_endpoint_exact_shape CHECK (
        endpoint_request_exact = (endpoint_request_commitment IS NOT NULL)
    ),
    ADD CONSTRAINT agent_model_observation_runtime_shape CHECK (
        (runtime_kind = 'legacy_0031'
         AND NOT endpoint_request_exact
         AND execution_profile_commitment IS NULL)
        OR
        (runtime_kind = 'client_provider_v1'
         AND execution_profile_commitment IS NOT NULL
         AND (status = 'explicit_failure' OR endpoint_request_exact)
         AND (provider_status NOT IN (
                'provider_unavailable',
                'provider_timeout',
                'invalid_structured_output'
              ) OR endpoint_request_exact))
    );

ALTER TABLE agent_model_invocation_projections
    ADD COLUMN runtime_kind text NOT NULL DEFAULT 'legacy_0031'
        CHECK (runtime_kind IN ('legacy_0031', 'client_provider_v1')),
    ADD COLUMN endpoint_request_exact boolean NOT NULL DEFAULT false,
    ADD COLUMN endpoint_request_commitment bytea
        CHECK (
            endpoint_request_commitment IS NULL
            OR octet_length(endpoint_request_commitment) = 32
        ),
    ADD COLUMN execution_profile_commitment bytea CHECK (
        execution_profile_commitment IS NULL
        OR octet_length(execution_profile_commitment) = 32
    ),
    ADD CONSTRAINT agent_model_projection_endpoint_exact_shape CHECK (
        endpoint_request_exact = (endpoint_request_commitment IS NOT NULL)
    ),
    ADD CONSTRAINT agent_model_projection_runtime_shape CHECK (
        (runtime_kind = 'legacy_0031'
         AND NOT endpoint_request_exact
         AND execution_profile_commitment IS NULL)
        OR
        (runtime_kind = 'client_provider_v1'
         AND execution_profile_commitment IS NOT NULL
         AND (status = 'explicit_failure' OR endpoint_request_exact))
    );

COMMENT ON COLUMN agent_model_attempt_observations.endpoint_request_commitment IS
    'Opaque SHA-256 commitment to the exact provider request observed by the authorized client/edge TCB; never provider configuration or plaintext.';
COMMENT ON COLUMN agent_model_invocation_projections.endpoint_request_commitment IS
    'Projection of the signed client/edge endpoint request commitment; NULL denotes a pre-0032 observation without this witness.';
COMMENT ON COLUMN agent_model_attempt_dispatches.runtime_kind IS
    'Trusted route discriminator: legacy 0031 or exact client-owned provider execution.';
COMMENT ON COLUMN agent_model_attempt_observations.execution_profile_commitment IS
    'Device-generated hiding commitment to the local execution profile; no provider/model/endpoint is server-visible.';
