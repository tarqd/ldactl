#!/usr/bin/env bash
set -euo pipefail

# Required env:
#   SECRET_PREFIX (defaults to 'launchdarkly-sdk')
#   SECRET_TAG_KEY (defaults to 'created-by')
#   SECRET_TAG_VALUE (defaults to 'ld-secret-sync')
#   SECRET_DELETE_TAG_KEY (defaults to 'ld-status')
#   SECRET_DELETE_TAG_VALUE (defaults to 'deleted')
# Optional:
#   ADD_DELETE_TIMESTAMP=1  # also tags ld-deleted-at=<epoch>

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

# Set defaults for required variables
SECRET_PREFIX="${SECRET_PREFIX:-launchdarkly-sdk}"
SECRET_TAG_KEY="${SECRET_TAG_KEY:-created-by}"
SECRET_TAG_VALUE="${SECRET_TAG_VALUE:-ld-secret-sync}"
SECRET_DELETE_TAG_KEY="${SECRET_DELETE_TAG_KEY:-ld-status}"
SECRET_DELETE_TAG_VALUE="${SECRET_DELETE_TAG_VALUE:-deleted}"

name_base="/${SECRET_PREFIX}/${project}/${env_key}"
server_name="${name_base}/server"
mobile_name="${name_base}/mobile"
client_name="${name_base}/client"

create_or_update() {
  local name="$1" value="$2"
  if [[ -z "$value" || "$value" == "null" ]]; then
    return 0
  fi
  
  if aws secretsmanager describe-secret --secret-id "$name" >/dev/null 2>&1; then
    aws secretsmanager put-secret-value --secret-id "$name" --secret-string "$value"
    # if it was previously "deleted", remove that tag now
    aws secretsmanager untag-resource --secret-id "$name" --tag-keys "$SECRET_DELETE_TAG_KEY" >/dev/null 2>&1 || true
  else
    aws secretsmanager create-secret \
      --name "$name" \
      --secret-string "$value" \
      --tags Key="${SECRET_TAG_KEY}",Value="${SECRET_TAG_VALUE}"
  fi
}

tag_as_deleted() {
  local name="$1"
  if aws secretsmanager describe-secret --secret-id "$name" >/dev/null 2>&1; then
    if [[ "${ADD_DELETE_TIMESTAMP:-}" == "1" ]]; then
      aws secretsmanager tag-resource \
        --secret-id "$name" \
        --tags Key="$SECRET_DELETE_TAG_KEY",Value="$SECRET_DELETE_TAG_VALUE" Key=ld-deleted-at,Value="$(date +%s)"
    else
      aws secretsmanager tag-resource \
        --secret-id "$name" \
        --tags Key="$SECRET_DELETE_TAG_KEY",Value="$SECRET_DELETE_TAG_VALUE"
    fi
  fi
}

echo handling event kind: "$event_kind"

case "$event_kind" in
  insert|update)
    create_or_update "$server_name" "$server_key"
    create_or_update "$mobile_name" "$mobile_key"
    create_or_update "$client_name" "$client_id"
    ;;
  delete)
    tag_as_deleted "$server_name"
    tag_as_deleted "$mobile_name"
    tag_as_deleted "$client_name"
    ;;
  *)
    echo "Error: Unknown event kind: $event_kind" >&2
    exit 1
    ;;
esac
