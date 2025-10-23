# Writing Custom Hooks for ldactl

This guide explains how to create custom hooks that integrate ldactl with your preferred secrets management systems, configuration files, or other services.

## Overview

ldactl hooks are executable scripts that receive LaunchDarkly environment configuration changes and can perform actions like:
- Syncing credentials to secrets managers (AWS Secrets Manager, HashiCorp Vault, etc.)
- Updating configuration files
- Restarting services
- Notifying external systems

## Hook Execution Modes

ldactl supports two execution modes for hooks:

### Environment Variables Mode (default)
When using `--exec-mode env` (or omitting the flag), ldactl sets environment variables that your hook can read:

```bash
ldactl --exec ./my-hook.sh
```

### JSON Mode
When using `--exec-mode change-json`, ldactl writes the change event as JSON to STDIN:

```bash
ldactl --exec-mode change-json --exec ./my-hook.sh
```

## Environment Variables

When using environment variable mode, ldactl sets the following variables:

### Event Information
- `LDAC_EVENT_KIND` - The type of event: `insert`, `update`, `delete`, or `initialized`

### Environment Data
- `LDAC_PROJECT_KEY` - The LaunchDarkly project key
- `LDAC_ENV_KEY` - The environment key
- `LDAC_ENV_ID` - The environment ID (client-side key)
- `LDAC_MOBILE_KEY` - The mobile SDK key
- `LDAC_SDK_KEY` - The server-side SDK key

## Basic Hook Structure

Here's a minimal hook template:

```bash
#!/usr/bin/env bash
set -euo pipefail

# Get event kind from environment variable
event_kind="${LDAC_EVENT_KIND:-}"

# Exit early if no event kind
if [[ -z "$event_kind" ]]; then
    echo "Error: LDAC_EVENT_KIND environment variable not set" >&2
    exit 1
fi

# Skip initialization events
if [[ "$event_kind" == "initialized" ]]; then
    exit 0
fi

# Extract environment data
project="${LDAC_PROJECT_KEY:-}"
env_key="${LDAC_ENV_KEY:-}"
client_id="${LDAC_ENV_ID:-}"
mobile_key="${LDAC_MOBILE_KEY:-}"
server_key="${LDAC_SDK_KEY:-}"

# Validate required values
if [[ -z "$project" || -z "$env_key" ]]; then
    echo "Error: Required environment variables not set" >&2
    exit 1
fi

# Handle different event types
case "$event_kind" in
  insert|update)
    echo "Creating/updating credentials for $project/$env_key"
    # Your logic here
    ;;
  delete)
    echo "Deleting credentials for $project/$env_key"
    # Your logic here
    ;;
  *)
    echo "Error: Unknown event kind: $event_kind" >&2
    exit 1
    ;;
esac
```

## JSON Mode Hook Structure

For JSON mode hooks, read the change event from STDIN:

```bash
#!/usr/bin/env bash
set -euo pipefail

# Read JSON from STDIN
json_input=$(cat)

# Extract event kind using jq
event_kind=$(echo "$json_input" | jq -r '.kind')

# Skip initialization events
if [[ "$event_kind" == "initialized" ]]; then
    exit 0
fi

# Extract environment data
project=$(echo "$json_input" | jq -r '.projectKey')
env_key=$(echo "$json_input" | jq -r '.environmentKey')
client_id=$(echo "$json_input" | jq -r '.environmentId')
mobile_key=$(echo "$json_input" | jq -r '.mobileKey')
server_key=$(echo "$json_input" | jq -r '.sdkKey')

# Handle different event types
case "$event_kind" in
  insert|update)
    echo "Creating/updating credentials for $project/$env_key"
    # Your logic here
    ;;
  delete)
    echo "Deleting credentials for $project/$env_key"
    # Your logic here
    ;;
esac
```

## Best Practices

### 1. Error Handling
- Use `set -euo pipefail` for strict error handling
- Validate required environment variables
- Handle missing or null values gracefully
- Exit with non-zero status on errors

### 2. Event Handling
- Always check for the `initialized` event and exit early
- Handle all event types (`insert`, `update`, `delete`)
- Provide meaningful error messages for unknown event types

### 3. Configuration
- Use environment variables for configuration with sensible defaults
- Document all required and optional configuration variables
- Validate configuration before processing

### 4. Idempotency
- Design hooks to be idempotent (safe to run multiple times)
- Check if resources exist before creating them
- Handle updates gracefully

### 5. Logging
- Log important operations for debugging
- Use stderr for error messages
- Include context (project, environment) in log messages

## Example: HashiCorp Vault Hook

Here's a complete example for syncing to HashiCorp Vault:

```bash
#!/usr/bin/env bash
set -euo pipefail

# Configuration
VAULT_ADDR="${VAULT_ADDR:-http://localhost:8200}"
VAULT_TOKEN="${VAULT_TOKEN:-}"
VAULT_PATH_PREFIX="${VAULT_PATH_PREFIX:-launchdarkly}"

# Get event kind
event_kind="${LDAC_EVENT_KIND:-}"

if [[ -z "$event_kind" ]]; then
    echo "Error: LDAC_EVENT_KIND environment variable not set" >&2
    exit 1
fi

if [[ "$event_kind" == "initialized" ]]; then
    exit 0
fi

# Extract environment data
project="${LDAC_PROJECT_KEY:-}"
env_key="${LDAC_ENV_KEY:-}"
client_id="${LDAC_ENV_ID:-}"
mobile_key="${LDAC_MOBILE_KEY:-}"
server_key="${LDAC_SDK_KEY:-}"

# Validate required values
if [[ -z "$project" || -z "$env_key" ]]; then
    echo "Error: Required environment variables not set" >&2
    exit 1
fi

if [[ -z "$VAULT_TOKEN" ]]; then
    echo "Error: VAULT_TOKEN environment variable not set" >&2
    exit 1
fi

# Build vault path
vault_path="${VAULT_PATH_PREFIX}/${project}/${env_key}"

create_or_update_secret() {
    local key="$1"
    local value="$2"
    
    if [[ -z "$value" || "$value" == "null" ]]; then
        return 0
    fi
    
    echo "Writing $key to vault path: $vault_path"
    vault kv put "$vault_path" "$key"="$value"
}

delete_secret() {
    echo "Deleting vault path: $vault_path"
    vault kv delete "$vault_path" || true
}

case "$event_kind" in
  insert|update)
    create_or_update_secret "server_key" "$server_key"
    create_or_update_secret "mobile_key" "$mobile_key"
    create_or_update_secret "client_id" "$client_id"
    ;;
  delete)
    delete_secret
    ;;
  *)
    echo "Error: Unknown event kind: $event_kind" >&2
    exit 1
    ;;
esac
```

## Testing Your Hook

### 1. Test with Mock Data
Create a test script that sets the required environment variables:

```bash
#!/usr/bin/env bash
export LDAC_EVENT_KIND="insert"
export LDAC_PROJECT_KEY="test-project"
export LDAC_ENV_KEY="test-env"
export LDAC_ENV_ID="test-client-id"
export LDAC_MOBILE_KEY="test-mobile-key"
export LDAC_SDK_KEY="test-server-key"

./my-hook.sh
```

### 2. Test Different Event Types
Test all event types:
- `insert` - New environment created
- `update` - Environment updated
- `delete` - Environment deleted
- `initialized` - Initial sync (should exit early)

### 3. Test Error Conditions
- Missing environment variables
- Invalid event types
- Network/service failures

## Integration Examples

### Docker Integration
```dockerfile
FROM alpine:latest
RUN apk add --no-cache bash curl jq
COPY hooks/my-hook.sh /hooks/
RUN chmod +x /hooks/my-hook.sh
ENTRYPOINT ["ldactl", "--exec", "/hooks/my-hook.sh"]
```

### Kubernetes CronJob
```yaml
apiVersion: batch/v1
kind: CronJob
metadata:
  name: ldactl-sync
spec:
  schedule: "0 */6 * * *"  # Every 6 hours
  jobTemplate:
    spec:
      template:
        spec:
          containers:
          - name: ldactl
            image: my-ldactl-hook:latest
            env:
            - name: LD_RELAY_AUTO_CONFIG_KEY
              valueFrom:
                secretKeyRef:
                  name: ldactl-secrets
                  key: relay-key
```

## Troubleshooting

### Common Issues

1. **Permission Denied**: Ensure your hook script is executable (`chmod +x`)
2. **Service Connection Failures**: Implement retry logic and proper error handling for external service calls

### Debugging Tips

- Add verbose logging to your hook
- Test with `--once` flag for one-time execution
- Use `echo` statements to verify environment variables
- Check ldactl logs for any errors in hook execution

## Available Example Hooks

Check the `hooks/` directory for complete examples:
- `write-aws-ssm.sh` - AWS Secrets Manager integration
- `write-gcp-secrets.sh` - Google Cloud Secret Manager
- `write-k8s-secrets.sh` - Kubernetes secrets
- `write-vault-kv.sh` - HashiCorp Vault KV
- `write-plist.sh` - macOS plist files
