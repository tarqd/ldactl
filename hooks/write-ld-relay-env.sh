#!/usr/bin/env bash
set -euo pipefail

# Required env:
#   LDAC_LDR_ENV_FILE (optional, defaults to stdout)
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
output_file="${LDAC_LDR_ENV_FILE:-}"

# Function to write environment variables
write_env_vars() {
    local action="$1"
    
    # Create environment variable names with project and environment keys
    # Replace hyphens with underscores to make valid environment variable names
    local project_safe="${project//-/_}"
    local env_key_safe="${env_key//-/_}"
    
    local env_var_prefix="LD_ENV_${project_safe}_${env_key_safe}"
    local mobile_var_prefix="LD_MOBILE_${project_safe}_${env_key_safe}"
    local client_var_prefix="LD_CLIENT_SIDE_ID_${project_safe}_${env_key_safe}"
    local table_var_prefix="LD_TABLE_NAME_${project_safe}_${env_key_safe}"
    local prefix_var_prefix="LD_PREFIX_${project_safe}_${env_key_safe}"
    local proj_var_prefix="LD_PROJ_KEY_${project_safe}_${env_key_safe}"
    
    if [[ "$action" == "delete" ]]; then
        # for delete do not write
    else
        # For insert/update events, write actual values
        
        # Write SDK key
        if [[ -n "$server_key" && "$server_key" != "null" ]]; then
            echo "${env_var_prefix}=${server_key}"
        else
            echo "${env_var_prefix}="
        fi
        
        # Write mobile key
        if [[ -n "$mobile_key" && "$mobile_key" != "null" ]]; then
            echo "${mobile_var_prefix}=${mobile_key}"
        else
            echo "${mobile_var_prefix}="
        fi
        
        # Write client side ID
        if [[ -n "$client_id" && "$client_id" != "null" ]]; then
            echo "${client_var_prefix}=${client_id}"
        else
            echo "${client_var_prefix}="
        fi
        
        # Write table name with CID replacement
        if [[ -n "${ENV_DATASTORE_TABLE_NAME:-}" ]]; then
            local table_name="${ENV_DATASTORE_TABLE_NAME}"
            if [[ -n "$client_id" && "$client_id" != "null" ]]; then
                table_name="${table_name//\$CID/$client_id}"
            fi
            echo "${table_var_prefix}=${table_name}"
        else
            echo "${table_var_prefix}="
        fi
        
        # Write prefix with CID replacement
        if [[ -n "${ENV_DATASTORE_PREFIX:-}" ]]; then
            local prefix="${ENV_DATASTORE_PREFIX}"
            if [[ -n "$client_id" && "$client_id" != "null" ]]; then
                prefix="${prefix//\$CID/$client_id}"
            fi
            echo "${prefix_var_prefix}=${prefix}"
        else
            echo "${prefix_var_prefix}="
        fi
        
        # Write project key
        echo "${proj_var_prefix}=${project}"
    fi
}

# Write to file or stdout
if [[ -n "$output_file" ]]; then
    write_env_vars "$event_kind" > "$output_file"
    echo "Environment variables written to: $output_file" >&2
else
    write_env_vars "$event_kind"
fi
