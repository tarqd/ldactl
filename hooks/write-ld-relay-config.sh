#!/usr/bin/env bash
set -euo pipefail

# Required env:
#   LDAC_LDR_CONFIG_FILE (optional, defaults to stdout)
#   ENV_DATASTORE_PREFIX (optional, for multi-environment support)
#   ENV_DATASTORE_TABLE_NAME (optional, for multi-environment support)

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

# Check for datastore configuration and warn if neither is set
if [[ -z "${ENV_DATASTORE_PREFIX:-}" && -z "${ENV_DATASTORE_TABLE_NAME:-}" ]]; then
    echo "Warning: You must set ENV_DATASTORE_PREFIX or ENV_DATASTORE_TABLE_NAME (for dynamodb) to support multiple environments" >&2
fi

# Determine output destination
output_file="${LDAC_LDR_CONFIG_FILE:-}"

# Function to write LD Relay configuration
write_config() {
    local action="$1"
    
    # Create environment name with project and environment keys
    # Replace hyphens with underscores to make valid environment names
    local project_safe="${project//-/_}"
    local env_key_safe="${env_key//-/_}"
    local env_name="${project_safe}_${env_key_safe}"
    
    if [[ "$action" == "delete" ]]; then
        # For delete events, write empty environment section
        echo "[environment \"${env_name}\"]"
        echo "sdkKey = \"\""
        echo "mobileKey = \"\""
        echo "envId = \"\""
        echo "prefix = \"\""
        echo "tableName = \"\""
    else
        # For insert/update events, write actual values
        
        # Write environment section header
        echo "[environment \"${env_name}\"]"
        
        # Write SDK key
        if [[ -n "$server_key" && "$server_key" != "null" ]]; then
            echo "sdkKey = \"${server_key}\""
        else
            echo "sdkKey = \"\""
        fi
        
        # Write mobile key
        if [[ -n "$mobile_key" && "$mobile_key" != "null" ]]; then
            echo "mobileKey = \"${mobile_key}\""
        else
            echo "mobileKey = \"\""
        fi
        
        # Write client side ID
        if [[ -n "$client_id" && "$client_id" != "null" ]]; then
            echo "envId = \"${client_id}\""
        else
            echo "envId = \"\""
        fi
        
        # Write prefix with CID replacement
        if [[ -n "${ENV_DATASTORE_PREFIX:-}" ]]; then
            local prefix="${ENV_DATASTORE_PREFIX}"
            if [[ -n "$client_id" && "$client_id" != "null" ]]; then
                prefix="${prefix//\$CID/$client_id}"
            fi
            echo "prefix = \"${prefix}\""
        else
            echo "prefix = \"\""
        fi
        
        # Write table name with CID replacement
        if [[ -n "${ENV_DATASTORE_TABLE_NAME:-}" ]]; then
            local table_name="${ENV_DATASTORE_TABLE_NAME}"
            if [[ -n "$client_id" && "$client_id" != "null" ]]; then
                table_name="${table_name//\$CID/$client_id}"
            fi
            echo "tableName = \"${table_name}\""
        else
            echo "tableName = \"\""
        fi
    fi
}

# Write to file or stdout
if [[ -n "$output_file" ]]; then
    write_config "$event_kind" > "$output_file"
    echo "LD Relay configuration written to: $output_file" >&2
else
    write_config "$event_kind"
fi
