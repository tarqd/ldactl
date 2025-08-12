#!/usr/bin/env bash
set -euo pipefail

# Required env:
#   VAULT_ADDR (Vault server address)
#   VAULT_TOKEN or VAULT_NAMESPACE with auth method
#   SECRET_PREFIX (defaults to 'launchdarkly-sdk')
#   VAULT_MOUNT_PATH (KV secrets engine mount path, defaults to 'secret')
#   VAULT_KV_VERSION (KV version, defaults to '2')
# Optional:
#   ADD_DELETE_TIMESTAMP=1  # also adds deletion timestamp metadata

# Get event kind from environment variable set by ldactl
event_kind="${LDAC_EVENT_KIND:-}"

# Exit early if no event kind or if it's initialized
if [[ -z "$event_kind" ]]; then
    echo "Error: LDAC_EVENT_KIND environment variable not set" >&2
    exit 1
fi

if [[ "$event_kind" == "initialized" ]]; then
    exit 0
fi

# Extract values from environment variables set by ldactl
project="${LDAC_PROJECT_KEY:-}"
env_key="${LDAC_ENV_KEY:-}"
client_id="${LDAC_ENV_ID:-}"
mobile_key="${LDAC_MOBILE_KEY:-}"
server_key="${LDAC_SDK_KEY:-}"

# Validate required values
if [[ -z "$project" || -z "$env_key" ]]; then
    echo "Error: Required environment variables LDAC_PROJECT_KEY and LDAC_ENV_KEY not set" >&2
    exit 1
fi

# Validate Vault connection
if [[ -z "${VAULT_ADDR:-}" ]]; then
    echo "Error: VAULT_ADDR environment variable not set" >&2
    exit 1
fi

# Get Vault mount path, defaulting to 'secret'
mount_path="${VAULT_MOUNT_PATH:-secret}"
kv_version="${VAULT_KV_VERSION:-2}"

# Set defaults for required variables
SECRET_PREFIX="${SECRET_PREFIX:-launchdarkly-sdk}"

# Build secret paths
path_base="${SECRET_PREFIX}/${project}/${env_key}"
server_path="${path_base}/server"
mobile_path="${path_base}/mobile"
client_path="${path_base}/client"

create_or_update_secret() {
  local path="$1" key="$2" value="$3"
  if [[ -z "$value" || "$value" == "null" ]]; then
    return 0
  fi
  
  if [[ "$kv_version" == "2" ]]; then
    # KV v2
    if vault kv put "${mount_path}/${path}" "$key=$value" >/dev/null 2>&1; then
      echo "Updated secret: ${mount_path}/${path}"
    else
      echo "Created secret: ${mount_path}/${path}"
    fi
  else
    # KV v1
    if vault write "${mount_path}/${path}" "$key=$value" >/dev/null 2>&1; then
      echo "Updated secret: ${mount_path}/${path}"
    else
      echo "Created secret: ${mount_path}/${path}"
    fi
  fi
}

delete_secret() {
  local path="$1"
  
  if [[ "$kv_version" == "2" ]]; then
    # KV v2 - add deletion metadata
    if [[ "${ADD_DELETE_TIMESTAMP:-}" == "1" ]]; then
      vault kv metadata put "${mount_path}/${path}" \
        custom_metadata="deleted=true,deleted-at=$(date +%s)" >/dev/null 2>&1 || true
    else
      vault kv metadata put "${mount_path}/${path}" \
        custom_metadata="deleted=true" >/dev/null 2>&1 || true
    fi
    echo "Marked secret as deleted: ${mount_path}/${path}"
  else
    # KV v1 - just delete the secret
    vault delete "${mount_path}/${path}" >/dev/null 2>&1 || true
    echo "Deleted secret: ${mount_path}/${path}"
  fi
}

case "$event_kind" in
  insert|update)
    create_or_update_secret "$server_path" "sdk-key" "$server_key"
    create_or_update_secret "$mobile_path" "mobile-key" "$mobile_key"
    create_or_update_secret "$client_path" "client-id" "$client_id"
    ;;
  delete)
    delete_secret "$server_path"
    delete_secret "$mobile_path"
    delete_secret "$client_path"
    ;;
  *)
    echo "Error: Unknown event kind: $event_kind" >&2
    exit 1
    ;;
esac
