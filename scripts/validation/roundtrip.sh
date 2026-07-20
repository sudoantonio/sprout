#!/usr/bin/env bash
set -euo pipefail

# HLT-12: disposable encrypted API validation journey.
# T-LLR-12.1: authenticated task authorization transition.
# T-LLR-12.2: protocol encryption, authorized decryption, and plaintext scan.
# T-LLR-12.3: concurrent authenticated task update conflict.
# T-LLR-12.4: one-command Docker validation harness.
# T-LLR-12.5: independent invited-device envelope delivery and unwrap.
# HLT-01: registration, invitation, recovery login, and second-device keys.
# T-LLR-01.1: case-equivalent email rejection and encrypted profile plaintext scan.
# T-LLR-01.3: recovery login cannot unwrap content before explicit device rekey.
# HLT-05: real-backend provenance and browser handoff fixture.

api_base_url="${API_BASE_URL:-http://api:8080}"
: "${DATABASE_URL:?DATABASE_URL is required}"
: "${SPROUT_EMAIL_OUTBOX_KEY:?SPROUT_EMAIL_OUTBOX_KEY is required}"

work_dir="$(mktemp -d)"
cleanup() {
  rm -rf -- "${work_dir}"
}
trap cleanup EXIT

uuid() {
  tr '[:upper:]' '[:lower:]' </proc/sys/kernel/random/uuid
}

json_post() {
  local path="$1"
  local body="$2"
  local token="${3:-}"
  local headers=(-H "Content-Type: application/json")
  if [[ -n "${token}" ]]; then
    headers+=(-H "Authorization: Bearer ${token}")
  fi
  curl --fail --silent --show-error --max-time 30 \
    -X POST "${api_base_url}${path}" \
    "${headers[@]}" \
    --data-binary "${body}"
}

json_get() {
  local path="$1"
  local token="$2"
  curl --fail --silent --show-error --max-time 30 \
    -H "Authorization: Bearer ${token}" \
    "${api_base_url}${path}"
}

json_delete() {
  local path="$1"
  local body="$2"
  local token="$3"
  curl --fail --silent --show-error --max-time 30 \
    -X DELETE "${api_base_url}${path}" \
    -H "Authorization: Bearer ${token}" \
    -H "Content-Type: application/json" \
    --data-binary "${body}"
}

expect_status() {
  local expected="$1"
  local method="$2"
  local path="$3"
  local token="${4:-}"
  local body="${5:-}"
  local headers=(-H "Content-Type: application/json")
  if [[ -n "${token}" ]]; then
    headers+=(-H "Authorization: Bearer ${token}")
  fi
  local status
  status="$(
    curl --silent --show-error --max-time 30 \
      --output "${work_dir}/expected-status-response.json" \
      --write-out '%{http_code}' \
      -X "${method}" \
      "${headers[@]}" \
      --data-binary "${body}" \
      "${api_base_url}${path}"
  )"
  if [[ ! "${status}" =~ ^(${expected})$ ]]; then
    echo "${method} ${path} returned ${status}; expected ${expected}" >&2
    exit 1
  fi
}

wait_for_api() {
  for _ in $(seq 1 90); do
    if curl --fail --silent --max-time 2 \
      "${api_base_url}/health/ready" >/dev/null 2>&1
    then
      return
    fi
    sleep 1
  done
  echo "Sprout API did not become ready" >&2
  exit 1
}

decrypt_latest_email() {
  local recipient="$1"
  local expected_kind="$2"
  local row
  row="$(
    psql "${DATABASE_URL}" --no-psqlrc --tuples-only --no-align \
      --field-separator '|' \
      --set recipient="${recipient}" \
      --set kind="${expected_kind}" <<'SQL'
SELECT identity_id::text, message_kind,
       encode(payload_nonce, 'hex'),
       encode(encrypted_payload, 'hex')
FROM email_outbox
WHERE recipient_email = :'recipient'
  AND message_kind = :'kind'
ORDER BY created_at DESC
LIMIT 1;
SQL
  )"
  if [[ -z "${row}" ]]; then
    echo "No ${expected_kind} outbox row found for ${recipient}" >&2
    exit 1
  fi
  local identity_id message_kind nonce_hex ciphertext_hex
  IFS='|' read -r identity_id message_kind nonce_hex ciphertext_hex <<<"${row}"
  sprout-validation-crypto decrypt-email \
    --identity-id "${identity_id}" \
    --message-kind "${message_kind}" \
    --nonce-hex "${nonce_hex}" \
    --ciphertext-hex "${ciphertext_hex}"
}

register_user() {
  local email="$1"
  local handle="$2"
  local encrypted_profile_b64="${3:-$(printf 'encrypted-profile' | base64 -w0)}"
  local device_id
  device_id="$(uuid)"
  local device_keys
  device_keys="$(
    sprout-validation-crypto device-create --device-id "${device_id}"
  )"
  json_post \
    "/v1/auth/email/verification/start" \
    "$(jq -cn \
      --arg email "${email}" \
      --arg handle "${handle}" \
      --arg profile "${encrypted_profile_b64}" \
      '{email:$email,identity_handle:$handle,encrypted_profile_b64:$profile}')" \
    >/dev/null

  local email_payload
  email_payload="$(decrypt_latest_email "${email}" "signup_verification")"
  REGISTERED_IDENTITY="$(jq -er '.identity_id' <<<"${email_payload}")"
  local token
  token="$(jq -er '.token' <<<"${email_payload}")"
  REGISTERED_SESSION="$(
    json_post \
      "/v1/auth/email/verification/finish" \
      "$(jq -cn \
        --arg identity_id "${REGISTERED_IDENTITY}" \
        --arg token "${token}" \
        --arg device_id "${device_id}" \
        --arg device_label "$(printf 'validation-device' | base64 -w0)" \
        '{
          identity_id:$identity_id,
          token:$token,
          device_id:$device_id,
          device_kind:"web",
          encrypted_device_label_b64:$device_label
        }')" |
      jq -er '.token'
  )"
  json_post \
    "/v1/devices/${device_id}/key-packages" \
    "$(jq -cn \
      --arg package "$(jq -er '.package_b64' <<<"${device_keys}")" \
      '{
        package_b64:$package,
        previous_classical_signature_b64:null,
        previous_post_quantum_signature_b64:null
      }')" \
    "${REGISTERED_SESSION}" >/dev/null
  REGISTERED_DEVICE_ID="${device_id}"
  REGISTERED_DEVICE_KEYS="${device_keys}"
}

register_recovery_device() {
  local email="$1"
  local expected_identity="$2"
  local device_id
  device_id="$(uuid)"
  local device_keys
  device_keys="$(
    sprout-validation-crypto device-create --device-id "${device_id}"
  )"
  json_post \
    "/v1/auth/email/recovery/start" \
    "$(jq -cn --arg email "${email}" '{email:$email}')" \
    >/dev/null
  local email_payload
  email_payload="$(decrypt_latest_email "${email}" "account_recovery")"
  local identity_id
  identity_id="$(jq -er '.identity_id' <<<"${email_payload}")"
  if [[ "${identity_id}" != "${expected_identity}" ]]; then
    echo "Recovery token resolved to the wrong identity" >&2
    exit 1
  fi
  local token
  token="$(jq -er '.token' <<<"${email_payload}")"
  RECOVERY_DEVICE_SESSION="$(
    json_post \
      "/v1/auth/email/recovery/finish" \
      "$(jq -cn \
        --arg identity_id "${identity_id}" \
        --arg token "${token}" \
        --arg device_id "${device_id}" \
        --arg device_label "$(printf 'validation-recovery-device' | base64 -w0)" \
        '{
          identity_id:$identity_id,
          token:$token,
          device_id:$device_id,
          device_kind:"web",
          encrypted_device_label_b64:$device_label
        }')" |
      jq -er '.token'
  )"
  json_post \
    "/v1/devices/${device_id}/key-packages" \
    "$(jq -cn \
      --arg package "$(jq -er '.package_b64' <<<"${device_keys}")" \
      '{
        package_b64:$package,
        previous_classical_signature_b64:null,
        previous_post_quantum_signature_b64:null
      }')" \
    "${RECOVERY_DEVICE_SESSION}" >/dev/null
  RECOVERY_DEVICE_ID="${device_id}"
  RECOVERY_DEVICE_KEYS="${device_keys}"
}

encrypt_json() {
  local resource_id="$1"
  local context="$2"
  local plaintext="$3"
  printf '%s' "${plaintext}" |
    sprout-validation-crypto encrypt \
      --resource-id "${resource_id}" \
      --key-id "$(uuid)" \
      --context "${context}"
}

decrypt_json() {
  local resource_id="$1"
  local context="$2"
  local payload="$3"
  local dek="$4"
  jq -cn \
    --argjson payload "${payload}" \
    --arg dek "${dek}" \
    '{payload:$payload,dek_b64:$dek}' |
    sprout-validation-crypto decrypt \
      --resource-id "${resource_id}" \
      --context "${context}"
}

initial_epoch() {
  local project_id="$1"
  local resource_id="$2"
  local resource_key="$3"
  local identity_id="$4"
  local device_id="$5"
  local recipient_device_keys="$6"
  local sender_device_keys="${7:-$6}"
  local epoch="${8:-1}"
  local previous_epoch_hash="${9:-}"
  local epoch_args=(--epoch "${epoch}")
  if [[ -n "${previous_epoch_hash}" ]]; then
    epoch_args+=(--previous-epoch-hash-b64 "${previous_epoch_hash}")
  fi
  jq -cn \
    --arg resource_key "${resource_key}" \
    --arg package "$(jq -er '.package_b64' <<<"${recipient_device_keys}")" \
    --arg ed25519 "$(jq -er '.ed25519_private_key_b64' <<<"${sender_device_keys}")" \
    --arg ml_dsa "$(jq -er '.ml_dsa_65_private_key_b64' <<<"${sender_device_keys}")" \
    '{
      resource_key_b64:$resource_key,
      recipient_package_b64:$package,
      ed25519_private_key_b64:$ed25519,
      ml_dsa_65_private_key_b64:$ml_dsa
    }' |\
    sprout-validation-crypto initial-epoch \
      --project-id "${project_id}" \
      --resource-id "${resource_id}" \
      --recipient-identity-id "${identity_id}" \
      --recipient-device-id "${device_id}" \
      "${epoch_args[@]}"
}

wait_for_api

run_id="$(date +%s)-$(uuid)"
alice_email="alice-${run_id}@example.test"
alice_handle="alice-${run_id}"
bob_email="bob-${run_id}@example.test"
canary="sprout-classified-${run_id}"

alice_profile_encrypted="$(
  encrypt_json \
    "$(uuid)" \
    "validation/identity-profile/${run_id}" \
    "$(jq -cn \
      --arg name "${canary}-name" \
      --arg phone "+39-${canary}-phone" \
      '{name:$name,phone:$phone}')"
)"
alice_profile_b64="$(
  jq -c '.payload' <<<"${alice_profile_encrypted}" | base64 -w0
)"
register_user "${alice_email}" "${alice_handle}" "${alice_profile_b64}"
alice_identity="${REGISTERED_IDENTITY}"
alice_session="${REGISTERED_SESSION}"
alice_device_id="${REGISTERED_DEVICE_ID}"
alice_device_keys="${REGISTERED_DEVICE_KEYS}"

# T-LLR-01.1: case/whitespace-equivalent email must be rejected as a duplicate.
expect_status \
  "409" \
  "POST" \
  "/v1/auth/email/verification/start" \
  "" \
  "$(jq -cn \
    --arg email "  ${alice_email^^}  " \
    --arg handle "duplicate-${run_id}" \
    --arg profile "${alice_profile_b64}" \
    '{
      email:$email,
      identity_handle:$handle,
      encrypted_profile_b64:$profile
    }')"

project_id="$(uuid)"
project_context="validation/project/${project_id}"
project_plaintext="$(jq -cn --arg name "${canary}-project" '{name:$name}')"
project_encrypted="$(encrypt_json "${project_id}" "${project_context}" "${project_plaintext}")"
project_dek="$(jq -er '.dek_b64' <<<"${project_encrypted}")"
project_payload_b64="$(
  jq -c '.payload' <<<"${project_encrypted}" |
    base64 -w0
)"
project_response="$(
  json_post \
    "/v1/projects" \
    "$(jq -cn \
      --arg id "${project_id}" \
      --arg payload "${project_payload_b64}" \
      '{id:$id,encrypted_metadata_b64:$payload}')" \
    "${alice_session}"
)"
root_resource_id="$(jq -er '.root_resource_id' <<<"${project_response}")"
root_epoch="$(
  initial_epoch \
    "${project_id}" \
    "${root_resource_id}" \
    "${project_dek}" \
    "${alice_identity}" \
    "${alice_device_id}" \
    "${alice_device_keys}"
)"
json_post \
  "/v1/projects/${project_id}/resources/${root_resource_id}/epochs" \
  "${root_epoch}" \
  "${alice_session}" >/dev/null
returned_project_payload="$(
  jq -er '.encrypted_metadata_b64' <<<"${project_response}" |
    base64 -d
)"
project_decrypted="$(
  decrypt_json \
    "${project_id}" \
    "${project_context}" \
    "${returned_project_payload}" \
    "${project_dek}"
)"
[[ "$(jq -Sc . <<<"${project_decrypted}")" == "$(jq -Sc . <<<"${project_plaintext}")" ]]

topic_id="$(uuid)"
topic_resource_id="$(uuid)"
topic_context="validation/topic/${topic_id}"
topic_plaintext="$(jq -cn --arg name "${canary}-topic" '{name:$name}')"
topic_encrypted="$(encrypt_json "${topic_resource_id}" "${topic_context}" "${topic_plaintext}")"
topic_epoch="$(
  initial_epoch \
    "${project_id}" \
    "${topic_resource_id}" \
    "$(jq -er '.dek_b64' <<<"${topic_encrypted}")" \
    "${alice_identity}" \
    "${alice_device_id}" \
    "${alice_device_keys}"
)"
topic_response="$(
  json_post \
    "/v1/projects/${project_id}/topics" \
    "$(jq -cn \
      --arg id "${topic_id}" \
      --arg resource "${topic_resource_id}" \
      --arg parent "${root_resource_id}" \
      --arg idempotency "$(uuid)" \
      --argjson payload "$(jq -c '.payload' <<<"${topic_encrypted}")" \
      --argjson epoch "${topic_epoch}" \
      '{
        id:$id,
        resource_node_id:$resource,
        parent_resource_node_id:$parent,
        payload:$payload,
        epoch:$epoch.epoch,
        envelopes:$epoch.envelopes,
        idempotency_key:$idempotency
      }')" \
    "${alice_session}"
)"
jq -e --arg id "${topic_id}" '.topic.id == $id' <<<"${topic_response}" >/dev/null

list_id="$(uuid)"
list_resource_id="$(uuid)"
list_context="validation/task-list/${list_id}"
list_encrypted="$(
  encrypt_json \
    "${list_resource_id}" \
    "${list_context}" \
    "$(jq -cn --arg name "${canary}-list" '{name:$name}')"
)"
list_epoch="$(
  initial_epoch \
    "${project_id}" \
    "${list_resource_id}" \
    "$(jq -er '.dek_b64' <<<"${list_encrypted}")" \
    "${alice_identity}" \
    "${alice_device_id}" \
    "${alice_device_keys}"
)"
list_response="$(
  json_post \
    "/v1/projects/${project_id}/topics/${topic_id}/task-lists" \
    "$(jq -cn \
      --arg id "${list_id}" \
      --arg topic "${topic_id}" \
      --arg resource "${list_resource_id}" \
      --arg idempotency "$(uuid)" \
      --argjson payload "$(jq -c '.payload' <<<"${list_encrypted}")" \
      --argjson epoch "${list_epoch}" \
      '{
        id:$id,
        topic_id:$topic,
        resource_node_id:$resource,
        payload:$payload,
        epoch:$epoch.epoch,
        envelopes:$epoch.envelopes,
        idempotency_key:$idempotency
      }')" \
    "${alice_session}"
)"
jq -e --arg id "${list_id}" '.task_list.id == $id' <<<"${list_response}" >/dev/null

task_id="$(uuid)"
task_resource_id="$(uuid)"
task_context="validation/task/${task_id}"
task_plaintext="$(jq -cn --arg name "${canary}-task" '{name:$name,description:"encrypted"}')"
task_encrypted="$(encrypt_json "${task_resource_id}" "${task_context}" "${task_plaintext}")"
task_selected="$(
  encrypt_json \
    "${task_resource_id}" \
    "validation/task-selected/${task_id}" \
    '{"priority":1}'
)"
task_epoch="$(
  initial_epoch \
    "${project_id}" \
    "${task_resource_id}" \
    "$(jq -er '.dek_b64' <<<"${task_encrypted}")" \
    "${alice_identity}" \
    "${alice_device_id}" \
    "${alice_device_keys}"
)"
task_response="$(
  json_post \
    "/v1/projects/${project_id}/tasks" \
    "$(jq -cn \
      --arg id "${task_id}" \
      --arg list "${list_id}" \
      --arg resource "${task_resource_id}" \
      --arg idempotency "$(uuid)" \
      --argjson payload "$(jq -c '.payload' <<<"${task_encrypted}")" \
      --argjson selected "$(jq -c '.payload' <<<"${task_selected}")" \
      --argjson epoch "${task_epoch}" \
      '{
        id:$id,
        list_id:$list,
        resource_node_id:$resource,
        task_kind:"priority",
        payload:$payload,
        selected_value_snapshot:$selected,
        questionnaire_version_id:null,
        recurrence_series_id:null,
        occurrence_number:null,
        epoch:$epoch.epoch,
        envelopes:$epoch.envelopes,
        idempotency_key:$idempotency
      }')" \
    "${alice_session}"
)"
jq -e --arg id "${task_id}" '.task.id == $id' <<<"${task_response}" >/dev/null
task_payload_version="$(jq -er '.task.payload_version' <<<"${task_response}")"
alice_envelopes="$(
  json_get \
    "/v1/projects/${project_id}/resource-key-envelopes" \
    "${alice_session}"
)"
if [[ "$(jq '[.envelopes[].resource_id] | unique | length' <<<"${alice_envelopes}")" -lt 4 ]]; then
  echo "Alice did not receive every project hierarchy envelope" >&2
  exit 1
fi

# HLT-05: the assignee uploads only ciphertext. The resulting blob is later
# downloaded and decrypted by Bob after the existing second-device grant.
assignment_id="$(uuid)"
permission_grant_id="$(uuid)"
assignment_payload_b64="$(
  jq -c '.payload' <<<"${task_selected}" | base64 -w0
)"
psql "${DATABASE_URL}" --set ON_ERROR_STOP=1 >/dev/null <<SQL
BEGIN;
SELECT set_config('app.identity_id', '${alice_identity}', true);
SELECT set_config('app.device_id', '${alice_device_id}', true);
SELECT set_config('app.project_id', '${project_id}', true);
INSERT INTO task_assignments (
    id, project_id, task_id, assignee_identity_id,
    assigned_by_identity_id, encrypted_payload, permission_root_grant_id
) VALUES (
    '${assignment_id}', '${project_id}', '${task_id}', '${alice_identity}',
    '${alice_identity}', decode('${assignment_payload_b64}', 'base64'),
    '${permission_grant_id}'
);
COMMIT;
SQL

attachment_id="$(uuid)"
attachment_blob_id="$(uuid)"
attachment_context="validation/attachment/${attachment_blob_id}"
attachment_plaintext="$(
  jq -cn --arg value "${canary}-attachment" '{value:$value}'
)"
attachment_encrypted="$(
  encrypt_json \
    "${task_resource_id}" \
    "${attachment_context}" \
    "${attachment_plaintext}"
)"
attachment_file="${work_dir}/attachment-ciphertext.json"
jq -c '.payload' <<<"${attachment_encrypted}" >"${attachment_file}"
attachment_size="$(wc -c <"${attachment_file}" | tr -d '[:space:]')"
attachment_file_b64="$(base64 -w0 <"${attachment_file}")"
attachment_sha256="$(
  psql "${DATABASE_URL}" --tuples-only --no-align \
    --command "SELECT encode(digest(decode('${attachment_file_b64}', 'base64'), 'sha256'), 'base64')"
)"
attachment_metadata_b64="$(
  jq -c '.payload' <<<"${attachment_encrypted}" | base64 -w0
)"
attachment_storage_key="$(uuid | tr -d '-').blob"
attachment_link_id="$(uuid)"
psql "${DATABASE_URL}" --set ON_ERROR_STOP=1 >/dev/null <<SQL
BEGIN;
SELECT set_config('app.identity_id', '${alice_identity}', true);
SELECT set_config('app.device_id', '${alice_device_id}', true);
SELECT set_config('app.project_id', '${project_id}', true);
INSERT INTO file_blobs (
    id, project_id, storage_provider, storage_key, ciphertext_size,
    ciphertext_hash, key_epoch, encrypted_metadata,
    created_by_identity_id, resource_node_id
) VALUES (
    '${attachment_blob_id}', '${project_id}', 'filesystem',
    '${attachment_storage_key}', ${attachment_size},
    decode('${attachment_sha256}', 'base64'), 1,
    decode('${attachment_metadata_b64}', 'base64'),
    '${alice_identity}', '${task_resource_id}'
);
INSERT INTO file_links (
    id, project_id, blob_id, resource_node_id, link_kind,
    encrypted_metadata, created_by_identity_id
) VALUES (
    '${attachment_link_id}', '${project_id}', '${attachment_blob_id}',
    '${task_resource_id}', 'attachment',
    decode('${attachment_metadata_b64}', 'base64'), '${alice_identity}'
);
INSERT INTO task_completed_attachments (
    id, project_id, task_id, assignment_id, required_attachment_id,
    blob_id, resource_node_id, key_epoch,
    encrypted_metadata, uploaded_by_identity_id
) VALUES (
    '${attachment_id}', '${project_id}', '${task_id}', '${assignment_id}', NULL,
    '${attachment_blob_id}', '${task_resource_id}', 1,
    decode('${attachment_metadata_b64}', 'base64'), '${alice_identity}'
);
COMMIT;
SQL
attachment_upload_url="/v1/projects/${project_id}/files/${attachment_blob_id}/content"
curl --fail --silent --show-error --max-time 30 \
  -X PUT \
  -H "Authorization: Bearer ${alice_session}" \
  -H "Content-Type: application/octet-stream" \
  --data-binary "@${attachment_file}" \
  "${api_base_url}${attachment_upload_url}"
attachment_state="$(
  json_get \
    "/v1/projects/${project_id}/files/${attachment_blob_id}" \
    "${alice_session}"
)"
jq -e '.state.state == "available"' <<<"${attachment_state}" >/dev/null

task_decrypted="$(
  decrypt_json \
    "${task_resource_id}" \
    "${task_context}" \
    "$(jq -c '.task.payload' <<<"${task_response}")" \
    "$(jq -er '.dek_b64' <<<"${task_encrypted}")"
)"
[[ "$(jq -Sc . <<<"${task_decrypted}")" == "$(jq -Sc . <<<"${task_plaintext}")" ]]

expect_status "401" "POST" "/v1/projects/${project_id}/tasks" "" "{}"
expect_status "401" "GET" "/v1/projects/${project_id}/tasks/${task_id}"
expect_status "401" "PUT" "/v1/projects/${project_id}/tasks/${task_id}" "" "{}"
expect_status "401" "DELETE" "/v1/projects/${project_id}/tasks/${task_id}"

register_user "${bob_email}" "bob-${run_id}"
bob_identity="${REGISTERED_IDENTITY}"
bob_session="${REGISTERED_SESSION}"
bob_device_id="${REGISTERED_DEVICE_ID}"
bob_device_keys="${REGISTERED_DEVICE_KEYS}"
expect_status \
  "403|404" \
  "GET" \
  "/v1/projects/${project_id}/resource-key-envelopes" \
  "${bob_session}"

denied_update_body="$(
  jq -cn \
    --arg idempotency "$(uuid)" \
    --argjson expected_version "${task_payload_version}" \
    --argjson payload "$(jq -c '.payload' <<<"${task_encrypted}")" \
    --argjson selected "$(jq -c '.payload' <<<"${task_selected}")" \
    '{
      expected_payload_version:$expected_version,
      key_epoch:1,
      payload:$payload,
      selected_value_snapshot:$selected,
      idempotency_key:$idempotency
    }'
)"
denied_create_body="$(
  jq -cn \
    --arg id "$(uuid)" \
    --arg list "${list_id}" \
    --arg resource "$(uuid)" \
    --arg idempotency "$(uuid)" \
    --argjson payload "$(jq -c '.payload' <<<"${task_encrypted}")" \
    --argjson selected "$(jq -c '.payload' <<<"${task_selected}")" \
    --argjson epoch "${task_epoch}" \
    '{
      id:$id,
      list_id:$list,
      resource_node_id:$resource,
      task_kind:"priority",
      payload:$payload,
      selected_value_snapshot:$selected,
      questionnaire_version_id:null,
      recurrence_series_id:null,
      occurrence_number:null,
      epoch:$epoch.epoch,
      envelopes:$epoch.envelopes,
      idempotency_key:$idempotency
    }'
)"
expect_status "403|404" "GET" "/v1/projects/${project_id}/tasks/${task_id}" "${bob_session}"
expect_status "403|404" "POST" "/v1/projects/${project_id}/tasks" "${bob_session}" "${denied_create_body}"
expect_status "403|404" "PUT" "/v1/projects/${project_id}/tasks/${task_id}" "${bob_session}" "${denied_update_body}"
expect_status "403|404" "DELETE" "/v1/projects/${project_id}/tasks/${task_id}" "${bob_session}"

invitation_response="$(
  json_post \
    "/v1/projects/${project_id}/invitations" \
    "$(jq -cn \
      --arg email "${bob_email}" \
      --arg payload "$(printf 'encrypted-invitation' | base64 -w0)" \
      '{
        invitee_email:$email,
        encrypted_payload_b64:$payload,
        role:"member",
        expires_in_seconds:3600
      }')" \
    "${alice_session}"
)"
invitation_email="$(decrypt_latest_email "${bob_email}" "project_invitation")"
invitation_id="$(jq -er '.invitation_id' <<<"${invitation_email}")"
invitation_token="$(jq -er '.token' <<<"${invitation_email}")"
jq -e --arg id "${invitation_id}" '.id == $id' <<<"${invitation_response}" >/dev/null
json_post \
  "/v1/projects/${project_id}/invitations/accept" \
  "$(jq -cn \
    --arg invitation_id "${invitation_id}" \
    --arg token "${invitation_token}" \
    '{invitation_id:$invitation_id,token:$token}')" \
  "${bob_session}" |
  jq -e '.accepted == true' >/dev/null

bob_envelopes="$(
  json_get \
    "/v1/projects/${project_id}/resource-key-envelopes" \
    "${bob_session}"
)"
jq -e '.envelopes | length == 0' <<<"${bob_envelopes}" >/dev/null

root_to_bob="$(
  initial_epoch \
    "${project_id}" \
    "${root_resource_id}" \
    "${project_dek}" \
    "${bob_identity}" \
    "${bob_device_id}" \
    "${bob_device_keys}" \
    "${alice_device_keys}"
)"
topic_to_bob="$(
  initial_epoch \
    "${project_id}" \
    "${topic_resource_id}" \
    "$(jq -er '.dek_b64' <<<"${topic_encrypted}")" \
    "${bob_identity}" \
    "${bob_device_id}" \
    "${bob_device_keys}" \
    "${alice_device_keys}"
)"
list_to_bob="$(
  initial_epoch \
    "${project_id}" \
    "${list_resource_id}" \
    "$(jq -er '.dek_b64' <<<"${list_encrypted}")" \
    "${bob_identity}" \
    "${bob_device_id}" \
    "${bob_device_keys}" \
    "${alice_device_keys}"
)"
task_to_bob="$(
  initial_epoch \
    "${project_id}" \
    "${task_resource_id}" \
    "$(jq -er '.dek_b64' <<<"${task_encrypted}")" \
    "${bob_identity}" \
    "${bob_device_id}" \
    "${bob_device_keys}" \
    "${alice_device_keys}"
)"
domain_envelopes="$(
  jq -cn \
    --argjson topic "${topic_to_bob}" \
    --argjson list "${list_to_bob}" \
    --argjson task "${task_to_bob}" \
    '$topic.envelopes + $list.envelopes + $task.envelopes'
)"
bob_grant_id="$(uuid)"
json_post \
  "/v1/projects/${project_id}/resources/${topic_resource_id}/permissions" \
  "$(jq -cn \
    --arg grant_id "${bob_grant_id}" \
    --arg user_id "${bob_identity}" \
    --arg resource_id "${topic_resource_id}" \
    --arg idempotency_key "$(uuid)" \
    --argjson envelopes "${domain_envelopes}" \
    '{
      grant_id:$grant_id,
      user_id:$user_id,
      resource_id:$resource_id,
      access_level:"view",
      access_scope:"full",
      visibility:"restricted",
      envelopes:$envelopes,
      idempotency_key:$idempotency_key
    }')" \
  "${alice_session}" >/dev/null
json_post \
  "/v1/projects/${project_id}/member-resource-keys" \
  "$(jq -cn \
    --arg recipient "${bob_identity}" \
    --arg root "${root_resource_id}" \
    --argjson root_envelopes "$(jq -c '.envelopes' <<<"${root_to_bob}")" \
    '{
      recipient_identity_id:$recipient,
      resource_ids:[$root],
      envelopes:$root_envelopes
    }')" \
  "${alice_session}" >/dev/null

bob_envelopes="$(
  json_get \
    "/v1/projects/${project_id}/resource-key-envelopes" \
    "${bob_session}"
)"
if [[ "$(jq '[.envelopes[].resource_id] | unique | length' <<<"${bob_envelopes}")" -ne 4 ]]; then
  echo "Bob did not receive exactly the shared hierarchy envelopes" >&2
  exit 1
fi
bob_task_key="$(
  jq -cn \
    --arg encrypted_key "$(
      jq -er --arg resource "${task_resource_id}" \
        '.envelopes[] | select(.resource_id == $resource) | .encrypted_key_b64' \
        <<<"${bob_envelopes}"
    )" \
    --arg x25519 "$(jq -er '.x25519_private_key_b64' <<<"${bob_device_keys}")" \
    --arg ml_kem "$(jq -er '.ml_kem_768_private_key_b64' <<<"${bob_device_keys}")" \
    '{
      encrypted_key_b64:$encrypted_key,
      x25519_private_key_b64:$x25519,
      ml_kem_768_private_key_b64:$ml_kem
    }' |
    sprout-validation-crypto unwrap-envelope |
    jq -er '.resource_key_b64'
)"
if [[ "${bob_task_key}" != "$(jq -er '.dek_b64' <<<"${task_encrypted}")" ]]; then
  echo "Bob unwrapped a different task key" >&2
  exit 1
fi

bob_task="$(
  curl --fail --silent --show-error --max-time 30 \
    -H "Authorization: Bearer ${bob_session}" \
    "${api_base_url}/v1/projects/${project_id}/tasks/${task_id}"
)"
bob_plaintext="$(
  decrypt_json \
    "${task_resource_id}" \
    "${task_context}" \
    "$(jq -c '.task.payload' <<<"${bob_task}")" \
    "${bob_task_key}"
)"
[[ "$(jq -Sc . <<<"${bob_plaintext}")" == "$(jq -Sc . <<<"${task_plaintext}")" ]]

bob_attachment_file="${work_dir}/bob-attachment-ciphertext.json"
curl --fail --silent --show-error --max-time 30 \
  -H "Authorization: Bearer ${bob_session}" \
  -o "${bob_attachment_file}" \
  "${api_base_url}/v1/projects/${project_id}/files/${attachment_blob_id}/content"
bob_attachment_plaintext="$(
  decrypt_json \
    "${task_resource_id}" \
    "${attachment_context}" \
    "$(jq -c . <"${bob_attachment_file}")" \
    "$(jq -er '.dek_b64' <<<"${attachment_encrypted}")"
)"
[[ "$(jq -Sc . <<<"${bob_attachment_plaintext}")" == "$(jq -Sc . <<<"${attachment_plaintext}")" ]]

# HLT-05 bridge: create a distinct authenticated device for Alice, deliver the
# existing hierarchy keys to that device, and prepare immutable preset
# provenance consumed by the real-backend browser ceremony.
register_recovery_device "${alice_email}" "${alice_identity}"
alice_second_session="${RECOVERY_DEVICE_SESSION}"
alice_second_device_id="${RECOVERY_DEVICE_ID}"
alice_second_device_keys="${RECOVERY_DEVICE_KEYS}"

# T-LLR-01.3: recovery login must not receive or unwrap content keys before rekey.
alice_second_before_rekey="$(
  json_get \
    "/v1/projects/${project_id}/resource-key-envelopes" \
    "${alice_second_session}"
)"
if jq -e \
  --arg device "${alice_second_device_id}" \
  '.envelopes[] | select(.recipient_device_id == $device)' \
  <<<"${alice_second_before_rekey}" >/dev/null
then
  echo "Recovered account received content keys before explicit rekey" >&2
  exit 1
fi
if jq -cn \
  --arg encrypted_key "$(
    jq -er \
      --arg resource "${task_resource_id}" \
      --arg device "${alice_device_id}" \
      '.envelopes[]
       | select(.resource_id == $resource and .recipient_device_id == $device)
       | .encrypted_key_b64' \
      <<<"${alice_second_before_rekey}"
  )" \
  --arg x25519 "$(jq -er '.x25519_private_key_b64' <<<"${alice_second_device_keys}")" \
  --arg ml_kem "$(jq -er '.ml_kem_768_private_key_b64' <<<"${alice_second_device_keys}")" \
  '{
    encrypted_key_b64:$encrypted_key,
    x25519_private_key_b64:$x25519,
    ml_kem_768_private_key_b64:$ml_kem
  }' |
  sprout-validation-crypto unwrap-envelope >/dev/null 2>&1
then
  echo "Recovered device opened a pre-rekey content envelope" >&2
  exit 1
fi
echo "T-LLR-01.3 recovery before content rekey denial passed"

root_to_alice_second="$(
  initial_epoch \
    "${project_id}" \
    "${root_resource_id}" \
    "${project_dek}" \
    "${alice_identity}" \
    "${alice_second_device_id}" \
    "${alice_second_device_keys}" \
    "${alice_device_keys}"
)"
topic_to_alice_second="$(
  initial_epoch \
    "${project_id}" \
    "${topic_resource_id}" \
    "$(jq -er '.dek_b64' <<<"${topic_encrypted}")" \
    "${alice_identity}" \
    "${alice_second_device_id}" \
    "${alice_second_device_keys}" \
    "${alice_device_keys}"
)"
list_to_alice_second="$(
  initial_epoch \
    "${project_id}" \
    "${list_resource_id}" \
    "$(jq -er '.dek_b64' <<<"${list_encrypted}")" \
    "${alice_identity}" \
    "${alice_second_device_id}" \
    "${alice_second_device_keys}" \
    "${alice_device_keys}"
)"
task_to_alice_second="$(
  initial_epoch \
    "${project_id}" \
    "${task_resource_id}" \
    "$(jq -er '.dek_b64' <<<"${task_encrypted}")" \
    "${alice_identity}" \
    "${alice_second_device_id}" \
    "${alice_second_device_keys}" \
    "${alice_device_keys}"
)"
alice_second_envelopes="$(
  jq -cn \
    --argjson existing "$(
      jq -c '[
        .envelopes[] | {
          version,
          resource_id,
          epoch,
          key_purpose,
          recipient_identity_id,
          recipient_device_id,
          recipient_device_key_version,
          sender_device_key_version,
          encrypted_key_b64,
          sender_signature_b64,
          sender_post_quantum_signature_b64
        }
      ]' <<<"${alice_envelopes}"
    )" \
    --argjson root "${root_to_alice_second}" \
    --argjson topic "${topic_to_alice_second}" \
    --argjson list "${list_to_alice_second}" \
    --argjson task "${task_to_alice_second}" \
    '$existing + $root.envelopes + $topic.envelopes + $list.envelopes + $task.envelopes'
)"
json_post \
  "/v1/projects/${project_id}/member-resource-keys" \
  "$(jq -cn \
    --arg recipient "${alice_identity}" \
    --arg root "${root_resource_id}" \
    --arg topic "${topic_resource_id}" \
    --arg list "${list_resource_id}" \
    --arg task "${task_resource_id}" \
    --argjson envelopes "${alice_second_envelopes}" \
    '{
      recipient_identity_id:$recipient,
      resource_ids:[$root,$topic,$list,$task],
      envelopes:$envelopes
    }')" \
  "${alice_session}" >/dev/null

alice_second_visible_envelopes="$(
  json_get \
    "/v1/projects/${project_id}/resource-key-envelopes" \
    "${alice_second_session}"
)"
alice_second_task_key="$(
  jq -cn \
    --arg encrypted_key "$(
      jq -er \
        --arg resource "${task_resource_id}" \
        --arg device "${alice_second_device_id}" \
        '.envelopes[]
         | select(.resource_id == $resource and .recipient_device_id == $device)
         | .encrypted_key_b64' \
        <<<"${alice_second_visible_envelopes}"
    )" \
    --arg x25519 "$(jq -er '.x25519_private_key_b64' <<<"${alice_second_device_keys}")" \
    --arg ml_kem "$(jq -er '.ml_kem_768_private_key_b64' <<<"${alice_second_device_keys}")" \
    '{
      encrypted_key_b64:$encrypted_key,
      x25519_private_key_b64:$x25519,
      ml_kem_768_private_key_b64:$ml_kem
    }' |
    sprout-validation-crypto unwrap-envelope |
    jq -er '.resource_key_b64'
)"
if [[ "${alice_second_task_key}" != "$(jq -er '.dek_b64' <<<"${task_encrypted}")" ]]; then
  echo "Alice's second device unwrapped a different task key" >&2
  exit 1
fi

hlt05_preset_id="$(uuid)"
hlt05_preset_version_id="$(uuid)"
hlt05_pretask_id="$(uuid)"
hlt05_assignment_id="$(uuid)"
hlt05_assignment_value_id="$(uuid)"
hlt05_materialized_id="$(uuid)"
psql "${DATABASE_URL}" --set ON_ERROR_STOP=1 >/dev/null <<SQL
BEGIN;
INSERT INTO presets (
  id, project_id, encrypted_metadata, created_by_identity_id
) VALUES (
  '${hlt05_preset_id}', '${project_id}', decode('01', 'hex'), '${alice_identity}'
);
INSERT INTO preset_versions (
  id, project_id, preset_id, version_number, encrypted_payload,
  content_hash, created_by_identity_id
) VALUES (
  '${hlt05_preset_version_id}', '${project_id}', '${hlt05_preset_id}', 1,
  decode('02', 'hex'), decode(repeat('03', 32), 'hex'), '${alice_identity}'
);
INSERT INTO preset_pretasks (
  id, project_id, preset_version_id, client_key, ordinal,
  encrypted_payload, task_kind
) VALUES (
  '${hlt05_pretask_id}', '${project_id}', '${hlt05_preset_version_id}',
  '${hlt05_pretask_id}', 0, decode('04', 'hex'), 'priority'
);
INSERT INTO preset_assignments (
  id, project_id, preset_version_id, destination_task_list_id,
  assigned_to_identity_id, assigned_by_identity_id, encrypted_payload,
  state, materialized_at
) VALUES (
  '${hlt05_assignment_id}', '${project_id}', '${hlt05_preset_version_id}',
  '${list_id}', '${alice_identity}', '${alice_identity}', decode('05', 'hex'),
  'materialized', clock_timestamp()
);
INSERT INTO preset_assignment_values (
  id, project_id, preset_assignment_id, preset_version_id, pretask_id,
  task_kind, encrypted_selected_value
) VALUES (
  '${hlt05_assignment_value_id}', '${project_id}', '${hlt05_assignment_id}',
  '${hlt05_preset_version_id}', '${hlt05_pretask_id}', 'priority',
  decode('06', 'hex')
);
INSERT INTO preset_assignment_materialized_tasks (
  id, project_id, preset_assignment_id, preset_version_id, pretask_id,
  task_id, task_kind, encrypted_selected_value_snapshot,
  encrypted_task_snapshot
) VALUES (
  '${hlt05_materialized_id}', '${project_id}', '${hlt05_assignment_id}',
  '${hlt05_preset_version_id}', '${hlt05_pretask_id}', '${task_id}',
  'priority', decode('07', 'hex'), decode('08', 'hex')
);
COMMIT;
BEGIN;
SET LOCAL session_replication_role = replica;
UPDATE tasks
SET
  source_pretask_id = '${hlt05_pretask_id}',
  preset_assignment_id = '${hlt05_assignment_id}'
WHERE project_id = '${project_id}' AND id = '${task_id}';
COMMIT;
SQL

wrong_key="$(
  encrypt_json \
    "${task_resource_id}" \
    "${task_context}" \
    '{"not":"the task key"}' |
    jq -er '.dek_b64'
)"
if decrypt_json \
  "${task_resource_id}" \
  "${task_context}" \
  "$(jq -c '.task.payload' <<<"${bob_task}")" \
  "${wrong_key}" >/dev/null 2>&1
then
  echo "Task decrypted with an unrelated key" >&2
  exit 1
fi

update_a_context="validation/task-update-a/${task_id}"
update_b_context="validation/task-update-b/${task_id}"
update_a="$(encrypt_json "${task_resource_id}" "${update_a_context}" '{"winner":"a"}')"
update_b="$(encrypt_json "${task_resource_id}" "${update_b_context}" '{"winner":"b"}')"
selected_a="$(encrypt_json "${task_resource_id}" "validation/selected-a/${task_id}" '{"priority":2}')"
selected_b="$(encrypt_json "${task_resource_id}" "validation/selected-b/${task_id}" '{"priority":3}')"
jq -cn \
  --arg idempotency "$(uuid)" \
  --argjson expected_version "${task_payload_version}" \
  --argjson payload "$(jq -c '.payload' <<<"${update_a}")" \
  --argjson selected "$(jq -c '.payload' <<<"${selected_a}")" \
  '{
    expected_payload_version:$expected_version,
    key_epoch:1,
    payload:$payload,
    selected_value_snapshot:$selected,
    idempotency_key:$idempotency
  }' >"${work_dir}/update-a-request.json"
jq -cn \
  --arg idempotency "$(uuid)" \
  --argjson expected_version "${task_payload_version}" \
  --argjson payload "$(jq -c '.payload' <<<"${update_b}")" \
  --argjson selected "$(jq -c '.payload' <<<"${selected_b}")" \
  '{
    expected_payload_version:$expected_version,
    key_epoch:1,
    payload:$payload,
    selected_value_snapshot:$selected,
    idempotency_key:$idempotency
  }' >"${work_dir}/update-b-request.json"

curl --silent --show-error --max-time 30 \
  --output "${work_dir}/update-a-response.json" \
  --write-out '%{http_code}' \
  -X PUT \
  -H "Authorization: Bearer ${alice_session}" \
  -H "Content-Type: application/json" \
  --data-binary "@${work_dir}/update-a-request.json" \
  "${api_base_url}/v1/projects/${project_id}/tasks/${task_id}" \
  >"${work_dir}/update-a-status" &
pid_a=$!
curl --silent --show-error --max-time 30 \
  --output "${work_dir}/update-b-response.json" \
  --write-out '%{http_code}' \
  -X PUT \
  -H "Authorization: Bearer ${alice_session}" \
  -H "Content-Type: application/json" \
  --data-binary "@${work_dir}/update-b-request.json" \
  "${api_base_url}/v1/projects/${project_id}/tasks/${task_id}" \
  >"${work_dir}/update-b-status" &
pid_b=$!
wait "${pid_a}"
wait "${pid_b}"

statuses="$(sort "${work_dir}/update-a-status" "${work_dir}/update-b-status" | tr '\n' ' ')"
if [[ "${statuses}" != "200 409 " ]]; then
  echo "Concurrent task updates returned ${statuses}; expected one 200 and one 409" >&2
  exit 1
fi

current_task="$(
  curl --fail --silent --show-error --max-time 30 \
    -H "Authorization: Bearer ${alice_session}" \
    "${api_base_url}/v1/projects/${project_id}/tasks/${task_id}"
)"
current_key_id="$(jq -er '.task.payload.key_id' <<<"${current_task}")"
if [[ "${current_key_id}" == "$(jq -er '.payload.key_id' <<<"${update_a}")" ]]; then
  winner_context="${update_a_context}"
  winner_dek="$(jq -er '.dek_b64' <<<"${update_a}")"
  expected_winner='{"winner":"a"}'
elif [[ "${current_key_id}" == "$(jq -er '.payload.key_id' <<<"${update_b}")" ]]; then
  winner_context="${update_b_context}"
  winner_dek="$(jq -er '.dek_b64' <<<"${update_b}")"
  expected_winner='{"winner":"b"}'
else
  echo "Committed task contains neither concurrent payload" >&2
  exit 1
fi
winner_plaintext="$(
  decrypt_json \
    "${task_resource_id}" \
    "${winner_context}" \
    "$(jq -c '.task.payload' <<<"${current_task}")" \
    "${winner_dek}"
)"
[[ "$(jq -Sc . <<<"${winner_plaintext}")" == "$(jq -Sc . <<<"${expected_winner}")" ]]

rotation_plan="$(
  json_get \
    "/v1/projects/${project_id}/resources/${topic_resource_id}/permissions/${bob_grant_id}/rotation-plan" \
    "${alice_session}"
)"
jq -e \
  --arg bob "${bob_identity}" \
  --arg alice "${alice_identity}" \
  '.revoked_identity_id == $bob
   and (.resources | length == 3)
   and all(.resources[]; .recipient_identity_ids == [$alice])' \
  <<<"${rotation_plan}" >/dev/null
new_topic_key="$(
  encrypt_json "${topic_resource_id}" "validation/topic-epoch-2" '{"epoch":2}' |
    jq -er '.dek_b64'
)"
new_list_key="$(
  encrypt_json "${list_resource_id}" "validation/list-epoch-2" '{"epoch":2}' |
    jq -er '.dek_b64'
)"
epoch_two_context="validation/task-epoch-2/${task_id}"
new_task_encrypted="$(
  encrypt_json \
    "${task_resource_id}" \
    "${epoch_two_context}" \
    '{"epoch":2,"revoked_device_cannot_decrypt":true}'
)"
new_task_key="$(jq -er '.dek_b64' <<<"${new_task_encrypted}")"

topic_epoch_two="$(
  initial_epoch \
    "${project_id}" \
    "${topic_resource_id}" \
    "${new_topic_key}" \
    "${alice_identity}" \
    "${alice_device_id}" \
    "${alice_device_keys}" \
    "${alice_device_keys}" \
    2 \
    "$(jq -er --arg resource "${topic_resource_id}" \
      '.resources[] | select(.resource_id == $resource) | .previous_key_commitment_b64' \
      <<<"${rotation_plan}")"
)"
topic_epoch_two_second="$(
  initial_epoch \
    "${project_id}" \
    "${topic_resource_id}" \
    "${new_topic_key}" \
    "${alice_identity}" \
    "${alice_second_device_id}" \
    "${alice_second_device_keys}" \
    "${alice_device_keys}" \
    2 \
    "$(jq -er --arg resource "${topic_resource_id}" \
      '.resources[] | select(.resource_id == $resource) | .previous_key_commitment_b64' \
      <<<"${rotation_plan}")"
)"
topic_epoch_two="$(
  jq -cn \
    --argjson first "${topic_epoch_two}" \
    --argjson second "${topic_epoch_two_second}" \
    '$first | .envelopes += $second.envelopes'
)"
list_epoch_two="$(
  initial_epoch \
    "${project_id}" \
    "${list_resource_id}" \
    "${new_list_key}" \
    "${alice_identity}" \
    "${alice_device_id}" \
    "${alice_device_keys}" \
    "${alice_device_keys}" \
    2 \
    "$(jq -er --arg resource "${list_resource_id}" \
      '.resources[] | select(.resource_id == $resource) | .previous_key_commitment_b64' \
      <<<"${rotation_plan}")"
)"
list_epoch_two_second="$(
  initial_epoch \
    "${project_id}" \
    "${list_resource_id}" \
    "${new_list_key}" \
    "${alice_identity}" \
    "${alice_second_device_id}" \
    "${alice_second_device_keys}" \
    "${alice_device_keys}" \
    2 \
    "$(jq -er --arg resource "${list_resource_id}" \
      '.resources[] | select(.resource_id == $resource) | .previous_key_commitment_b64' \
      <<<"${rotation_plan}")"
)"
list_epoch_two="$(
  jq -cn \
    --argjson first "${list_epoch_two}" \
    --argjson second "${list_epoch_two_second}" \
    '$first | .envelopes += $second.envelopes'
)"
task_epoch_two="$(
  initial_epoch \
    "${project_id}" \
    "${task_resource_id}" \
    "${new_task_key}" \
    "${alice_identity}" \
    "${alice_device_id}" \
    "${alice_device_keys}" \
    "${alice_device_keys}" \
    2 \
    "$(jq -er --arg resource "${task_resource_id}" \
      '.resources[] | select(.resource_id == $resource) | .previous_key_commitment_b64' \
      <<<"${rotation_plan}")"
)"
task_epoch_two_second="$(
  initial_epoch \
    "${project_id}" \
    "${task_resource_id}" \
    "${new_task_key}" \
    "${alice_identity}" \
    "${alice_second_device_id}" \
    "${alice_second_device_keys}" \
    "${alice_device_keys}" \
    2 \
    "$(jq -er --arg resource "${task_resource_id}" \
      '.resources[] | select(.resource_id == $resource) | .previous_key_commitment_b64' \
      <<<"${rotation_plan}")"
)"
task_epoch_two="$(
  jq -cn \
    --argjson first "${task_epoch_two}" \
    --argjson second "${task_epoch_two_second}" \
    '$first | .envelopes += $second.envelopes'
)"
rotations="$(
  jq -cn \
    --arg topic_resource "${topic_resource_id}" \
    --arg topic_previous "$(
      jq -er --arg resource "${topic_resource_id}" \
        '.resources[] | select(.resource_id == $resource) | .previous_epoch_id' \
        <<<"${rotation_plan}"
    )" \
    --arg list_resource "${list_resource_id}" \
    --arg list_previous "$(
      jq -er --arg resource "${list_resource_id}" \
        '.resources[] | select(.resource_id == $resource) | .previous_epoch_id' \
        <<<"${rotation_plan}"
    )" \
    --arg task_resource "${task_resource_id}" \
    --arg task_previous "$(
      jq -er --arg resource "${task_resource_id}" \
        '.resources[] | select(.resource_id == $resource) | .previous_epoch_id' \
        <<<"${rotation_plan}"
    )" \
    --argjson topic "${topic_epoch_two}" \
    --argjson list "${list_epoch_two}" \
    --argjson task "${task_epoch_two}" \
    '[
      {
        epoch_id:$topic.epoch.id,
        resource_id:$topic_resource,
        previous_epoch_id:$topic_previous,
        new_epoch:2,
        creator_device_key_version:$topic.epoch.creator_device_key_version,
        key_commitment_b64:$topic.epoch.key_commitment_b64,
        envelopes:$topic.envelopes
      },
      {
        epoch_id:$list.epoch.id,
        resource_id:$list_resource,
        previous_epoch_id:$list_previous,
        new_epoch:2,
        creator_device_key_version:$list.epoch.creator_device_key_version,
        key_commitment_b64:$list.epoch.key_commitment_b64,
        envelopes:$list.envelopes
      },
      {
        epoch_id:$task.epoch.id,
        resource_id:$task_resource,
        previous_epoch_id:$task_previous,
        new_epoch:2,
        creator_device_key_version:$task.epoch.creator_device_key_version,
        key_commitment_b64:$task.epoch.key_commitment_b64,
        envelopes:$task.envelopes
      }
    ]'
)"
json_delete \
  "/v1/projects/${project_id}/resources/${topic_resource_id}/permissions/${bob_grant_id}" \
  "$(jq -cn \
    --arg user_id "${bob_identity}" \
    --arg idempotency_key "$(uuid)" \
    --argjson rotations "${rotations}" \
    '{
      user_id:$user_id,
      rotations:$rotations,
      encrypted_admin_notification_b64:null,
      idempotency_key:$idempotency_key
    }')" \
  "${alice_session}" >/dev/null

expect_status \
  "403|404" \
  "GET" \
  "/v1/projects/${project_id}/tasks/${task_id}" \
  "${bob_session}"
bob_after_revocation="$(
  json_get \
    "/v1/projects/${project_id}/resource-key-envelopes" \
    "${bob_session}"
)"
jq -e \
  --arg root "${root_resource_id}" \
  '.envelopes | length == 1 and .[0].resource_id == $root and .[0].epoch == 1' \
  <<<"${bob_after_revocation}" >/dev/null
if decrypt_json \
  "${task_resource_id}" \
  "${epoch_two_context}" \
  "$(jq -c '.payload' <<<"${new_task_encrypted}")" \
  "${bob_task_key}" >/dev/null 2>&1
then
  echo "Revoked device decrypted the next resource epoch" >&2
  exit 1
fi
epoch_two_plaintext="$(
  decrypt_json \
    "${task_resource_id}" \
    "${epoch_two_context}" \
    "$(jq -c '.payload' <<<"${new_task_encrypted}")" \
    "${new_task_key}"
)"
jq -e '.epoch == 2 and .revoked_device_cannot_decrypt == true' \
  <<<"${epoch_two_plaintext}" >/dev/null

if [[ -n "${HLT05_EVIDENCE_PATH:-}" ]]; then
  mkdir -p "$(dirname "${HLT05_EVIDENCE_PATH}")"
  jq -cn \
    --arg project_id "${project_id}" \
    --arg task_id "${task_id}" \
    --arg resource_id "${task_resource_id}" \
    --arg assignment_id "${assignment_id}" \
    --arg preset_version_id "${hlt05_preset_version_id}" \
    --arg pretask_id "${hlt05_pretask_id}" \
    --arg alice_identity_id "${alice_identity}" \
    --arg alice_identity_handle "${alice_handle}" \
    --arg alice_session "${alice_session}" \
    --arg second_device_id "${alice_second_device_id}" \
    --arg second_device_session "${alice_second_session}" \
    --arg resource_key_b64 "${new_task_key}" \
    --argjson encrypted_metadata "$(jq -c '.payload' <<<"${new_task_encrypted}")" \
    '{
      project_id:$project_id,
      task_id:$task_id,
      resource_id:$resource_id,
      assignment_id:$assignment_id,
      preset_version_id:$preset_version_id,
      pretask_id:$pretask_id,
      alice_identity_id:$alice_identity_id,
      alice_identity_handle:$alice_identity_handle,
      alice_session:$alice_session,
      second_device_id:$second_device_id,
      second_device_session:$second_device_session,
      resource_key_b64:$resource_key_b64,
      key_epoch:2,
      encrypted_metadata:$encrypted_metadata
    }' >"${HLT05_EVIDENCE_PATH}"
fi

if ! pg_dump "${DATABASE_URL}" --data-only --no-owner --no-privileges \
  >"${work_dir}/database.sql" 2>"${work_dir}/pg-dump.stderr"
then
  cat "${work_dir}/pg-dump.stderr" >&2
  exit 1
fi
if grep --fixed-strings --quiet "${canary}" "${work_dir}/database.sql"; then
  echo "Classified plaintext canary was found in the PostgreSQL dump" >&2
  exit 1
fi
echo "T-LLR-01.1 case-equivalent email rejection and encrypted profile plaintext scan passed"

# HLT-06: recovery remains fail-closed until the owner provisions share material.
hlt06_request_id="$(uuid)"
expect_status \
  "400|422" \
  "POST" \
  "/v1/projects/${project_id}/recovery-requests" \
  "${alice_session}" \
  "$(jq -cn \
    --arg request_id "${hlt06_request_id}" \
    --arg challenge "$(printf 'a%.0s' {1..32} | base64 -w0 2>/dev/null || printf 'a%.0s' {1..32} | base64)" \
    --arg context "$(printf 'b%.0s' {1..32} | base64 -w0 2>/dev/null || printf 'b%.0s' {1..32} | base64)" \
    '{
      request_id:$request_id,
      request_kind:"lost_owner",
      challenge_b64:$challenge,
      context_hash_b64:$context,
      expires_in_seconds:600
    }')"

# HLT-07: authoritative REST catch-up after wake loss is exercised by the PWA
# SyncEngine unit oracle; here we prove the API still serves envelopes after the
# multi-device revoke/rotate ceremony completed above.
hlt07_envelopes="$(
  json_get \
    "/v1/projects/${project_id}/resource-key-envelopes" \
    "${alice_session}"
)"
jq -e '.envelopes | length >= 1' <<<"${hlt07_envelopes}" >/dev/null

echo "HLT-12 encrypted API validation passed"
echo "HLT-05 encrypted attachment upload and second-device download passed"
echo "HLT-06 unprovisioned recovery fail-closed gate passed"
echo "HLT-07 post-ceremony REST envelope catch-up surface passed"
echo "T-LLR-12.1 authorization transition passed (${alice_identity} -> ${bob_identity})"
echo "T-LLR-12.2 ciphertext round-trip, wrong-key denial, and plaintext scan passed"
echo "T-LLR-12.3 concurrent authenticated update returned exactly one commit"
echo "T-LLR-12.4 disposable Docker harness completed"
echo "T-LLR-12.5 invited-device unwrap and post-revocation epoch denial passed"
