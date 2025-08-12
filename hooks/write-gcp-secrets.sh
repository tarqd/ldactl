#!/usr/bin/env bash
set -euo pipefail

# Required env:
#   GOOGLE_APPLICATION_CREDENTIALS or gcloud auth
#   SECRET_PREFIX (defaults to 'launchdarkly-sdk')
#   PROJECT_ID (GCP project ID, defaults to gcloud config project)
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

# Get GCP project ID, defaulting to gcloud config
gcp_project="${PROJECT_ID:-$(gcloud config get-value project 2>/dev/null || echo '')}"
if [[ -z "$gcp_project" ]]; then
    echo "Error: PROJECT_ID environment variable not set and gcloud not configured" >&2
    exit 1
fi

# Set defaults for required variables
SECRET_PREFIX="${SECRET_PREFIX:-launchdarkly-sdk}"

name_base="${SECRET_PREFIX}-${project}-${env_key}"
server_name="${name_base}-server"
mobile_name="${name_base}-mobile"
client_name="${name_base}-client"

create_or_update() {
  local name="$1" value="$2"
  if [[ -z "$value" || "$value" == "null" ]]; then
    return 0
  fi
  
  # Check if secret exists
  if gcloud secrets describe "$name" --project="$gcp_project" >/dev/null 2>&1; then
    # Update existing secret
    echo "$value" | gcloud secrets versions add "$name" --data-file=- --project="$gcp_project"
    echo "Updated secret: $name"
  else
    # Create new secret
    echo "$value" | gcloud secrets create "$name" --data-file=- --project="$gcp_project"
    echo "Created secret: $name"
  fi
}

delete_secret() {
  local name="$1"
  if gcloud secrets describe "$name" --project="$gcp_project" >/dev/null 2>&1; then
    if [[ "${ADD_DELETE_TIMESTAMP:-}" == "1" ]]; then
      # Add deletion timestamp metadata
      gcloud secrets update "$name" --update-labels="deleted=true,deleted-at=$(date +%s)" --project="$gcp_project" >/dev/null 2>&1 || true
    else
      # Just mark as deleted
      gcloud secrets update "$name" --update-labels="deleted=true" --project="$gcp_project" >/dev/null 2>&1 || true
    fi
    echo "Marked secret as deleted: $name"
  fi
}

case "$event_kind" in
  insert|update)
    create_or_update "$server_name" "$server_key"
    create_or_update "$mobile_name" "$mobile_key"
    create_or_update "$client_name" "$client_id"
    ;;
  delete)
    delete_secret "$server_name"
    delete_secret "$mobile_name"
    delete_secret "$client_name"
    ;;
  *)
    echo "Error: Unknown event kind: $event_kind" >&2
    exit 1
    ;;
esac
