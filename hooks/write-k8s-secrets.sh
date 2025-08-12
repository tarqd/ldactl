#!/usr/bin/env bash
set -euo pipefail

# Required env:
#   KUBECONFIG or kubectl context configured
#   SECRET_PREFIX (defaults to 'launchdarkly-sdk')
#   NAMESPACE (Kubernetes namespace, defaults to 'default')
#   CLUSTER_NAME (optional, for secret naming)
# Optional:
#   ADD_DELETE_TIMESTAMP=1  # also adds deletion timestamp annotation

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

# Get namespace, defaulting to 'default'
namespace="${NAMESPACE:-default}"

# Set defaults for required variables
SECRET_PREFIX="${SECRET_PREFIX:-launchdarkly-sdk}"

# Build secret names
name_base="${SECRET_PREFIX}-${project}-${env_key}"
if [[ -n "${CLUSTER_NAME:-}" ]]; then
    name_base="${name_base}-${CLUSTER_NAME}"
fi

server_secret_name="${name_base}-server"
mobile_secret_name="${name_base}-mobile"
client_secret_name="${name_base}-client"

create_or_update_secret() {
  local name="$1" key="$2" value="$3"
  if [[ -z "$value" || "$value" == "null" ]]; then
    return 0
  fi
  
  # Create temporary file for the secret
  local temp_file=$(mktemp)
  trap "rm -f $temp_file" EXIT
  
  # Check if secret exists
  if kubectl get secret "$name" -n "$namespace" >/dev/null 2>&1; then
    # Update existing secret
    kubectl patch secret "$name" -n "$namespace" -p="{\"data\":{\"$key\":\"$(echo -n "$value" | base64)\"}}"
    echo "Updated secret: $name"
  else
    # Create new secret
    kubectl create secret generic "$name" \
      --from-literal="$key=$value" \
      --namespace="$namespace" \
      --dry-run=client -o yaml | kubectl apply -f -
    echo "Created secret: $name"
  fi
}

delete_secret() {
  local name="$1"
  if kubectl get secret "$name" -n "$namespace" >/dev/null 2>&1; then
    if [[ "${ADD_DELETE_TIMESTAMP:-}" == "1" ]]; then
      # Add deletion timestamp annotation
      kubectl annotate secret "$name" -n "$namespace" \
        "deleted=true" \
        "deleted-at=$(date +%s)" \
        --overwrite >/dev/null 2>&1 || true
    else
      # Just mark as deleted
      kubectl annotate secret "$name" -n "$namespace" \
        "deleted=true" \
        --overwrite >/dev/null 2>&1 || true
    fi
    echo "Marked secret as deleted: $name"
  fi
}

case "$event_kind" in
  insert|update)
    create_or_update_secret "$server_secret_name" "sdk-key" "$server_key"
    create_or_update_secret "$mobile_secret_name" "mobile-key" "$mobile_key"
    create_or_update_secret "$client_secret_name" "client-id" "$client_id"
    ;;
  delete)
    delete_secret "$server_secret_name"
    delete_secret "$mobile_secret_name"
    delete_secret "$client_secret_name"
    ;;
  *)
    echo "Error: Unknown event kind: $event_kind" >&2
    exit 1
    ;;
esac
