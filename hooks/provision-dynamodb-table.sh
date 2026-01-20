#!/usr/bin/env bash
set -euo pipefail

# DynamoDB Table Provisioning Hook for LaunchDarkly
#
# This hook provisions a DynamoDB table for each LaunchDarkly environment
# if it does not already exist. The table is created with the format: ld-$clientsideid
#
# Table Schema (required by LaunchDarkly SDK):
#   - Partition Key: "namespace" (String)
#   - Sort Key: "key" (String)
#
# Configuration (environment variables):
#   TABLE_NAME_PREFIX   - Prefix for table name (default: "ld-")
#   BILLING_MODE        - DynamoDB billing mode (default: "PAY_PER_REQUEST")
#                         Options: "PAY_PER_REQUEST" or "PROVISIONED"
#   READ_CAPACITY       - Read capacity units (default: 5, only used if BILLING_MODE=PROVISIONED)
#   WRITE_CAPACITY      - Write capacity units (default: 5, only used if BILLING_MODE=PROVISIONED)
#   AWS_REGION          - AWS region (default: uses AWS CLI default region)
#   DELETE_TABLE        - Set to "1" to delete table on delete events (default: disabled)
#   TABLE_TAGS          - JSON string of tags to apply to table (optional)
#                         Example: '[{"Key":"Environment","Value":"Production"}]'
#
# Requirements:
#   - AWS CLI must be installed and configured
#   - IAM permissions: dynamodb:CreateTable, dynamodb:DescribeTable, dynamodb:ListTagsOfResource, dynamodb:TagResource
#   - IAM permissions for delete (if DELETE_TABLE=1): dynamodb:DeleteTable

# Get event kind from environment variable set by ldactl
event_kind="${LDAC_EVENT_KIND:-}"

# Exit early if no event kind
if [[ -z "$event_kind" ]]; then
    echo "Error: LDAC_EVENT_KIND environment variable not set" >&2
    exit 1
fi

# Exit early for initialized events
if [[ "$event_kind" == "initialized" ]]; then
    exit 0
fi

# Extract values from environment variables set by ldactl
project="${LDAC_PROJECT_KEY:-}"
env_key="${LDAC_ENV_KEY:-}"
client_id="${LDAC_ENV_ID:-}"

# Validate required values
if [[ -z "$project" || -z "$env_key" || -z "$client_id" ]]; then
    echo "Error: Required environment variables not set (LDAC_PROJECT_KEY, LDAC_ENV_KEY, LDAC_ENV_ID)" >&2
    exit 1
fi

# Configuration with defaults
table_prefix="${TABLE_NAME_PREFIX:-ld-}"
table_name="${table_prefix}${client_id}"
billing_mode="${BILLING_MODE:-PAY_PER_REQUEST}"
read_capacity="${READ_CAPACITY:-5}"
write_capacity="${WRITE_CAPACITY:-5}"
aws_region_arg=""
if [[ -n "${AWS_REGION:-}" ]]; then
    aws_region_arg="--region ${AWS_REGION}"
fi

# Function to check if table exists
table_exists() {
    local table=$1
    if aws dynamodb describe-table --table-name "$table" $aws_region_arg &>/dev/null; then
        return 0
    else
        return 1
    fi
}

# Function to create DynamoDB table
create_table() {
    local table=$1

    echo "Creating DynamoDB table: $table"

    # Build attribute definitions
    local attr_defs='[{"AttributeName":"namespace","AttributeType":"S"},{"AttributeName":"key","AttributeType":"S"}]'

    # Build key schema
    local key_schema='[{"AttributeName":"namespace","KeyType":"HASH"},{"AttributeName":"key","KeyType":"RANGE"}]'

    # Build create-table command based on billing mode
    if [[ "$billing_mode" == "PROVISIONED" ]]; then
        aws dynamodb create-table \
            --table-name "$table" \
            --attribute-definitions "$attr_defs" \
            --key-schema "$key_schema" \
            --billing-mode PROVISIONED \
            --provisioned-throughput "ReadCapacityUnits=${read_capacity},WriteCapacityUnits=${write_capacity}" \
            $aws_region_arg \
            --output json > /dev/null
    else
        aws dynamodb create-table \
            --table-name "$table" \
            --attribute-definitions "$attr_defs" \
            --key-schema "$key_schema" \
            --billing-mode PAY_PER_REQUEST \
            $aws_region_arg \
            --output json > /dev/null
    fi

    # Wait for table to become active
    echo "Waiting for table $table to become active..."
    aws dynamodb wait table-exists --table-name "$table" $aws_region_arg

    # Apply tags if specified
    if [[ -n "${TABLE_TAGS:-}" ]]; then
        local table_arn=$(aws dynamodb describe-table --table-name "$table" $aws_region_arg --query 'Table.TableArn' --output text)
        echo "Applying tags to table $table"
        aws dynamodb tag-resource \
            --resource-arn "$table_arn" \
            --tags "$TABLE_TAGS" \
            $aws_region_arg
    fi

    echo "Table $table created successfully"
}

# Function to delete DynamoDB table
delete_table() {
    local table=$1

    echo "Deleting DynamoDB table: $table"
    aws dynamodb delete-table --table-name "$table" $aws_region_arg --output json > /dev/null
    echo "Table $table deleted successfully"
}

# Handle different event types
case "$event_kind" in
    insert|update)
        if table_exists "$table_name"; then
            echo "DynamoDB table already exists: $table_name"
        else
            create_table "$table_name"
        fi
        ;;

    delete)
        # Only delete table if explicitly configured
        if [[ "${DELETE_TABLE:-0}" == "1" ]]; then
            if table_exists "$table_name"; then
                delete_table "$table_name"
            else
                echo "DynamoDB table does not exist: $table_name (nothing to delete)"
            fi
        else
            echo "Skipping table deletion for $table_name (set DELETE_TABLE=1 to enable)"
        fi
        ;;

    *)
        echo "Error: Unknown event kind: $event_kind" >&2
        exit 1
        ;;
esac
