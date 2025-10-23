# LDACTL Hooks Documentation

This document describes the available hooks for LDACTL (LaunchDarkly AutoConfig Tool). Each hook handles different event types (insert, update, delete) and stores LaunchDarkly SDK credentials in various systems.

## Available Hooks

### write-aws-ssm.sh
**Purpose**: Stores LaunchDarkly SDK credentials in AWS Systems Manager Parameter Store  
**Method**: AWS CLI (`aws secretsmanager`)  
**Configuration**:
- `SECRET_PREFIX` (default: `launchdarkly-sdk`) - Prefix for secret names
- `SECRET_TAG_KEY` (default: `created-by`) - Tag key for created secrets
- `SECRET_TAG_VALUE` (default: `ld-secret-sync`) - Tag value for created secrets
- `SECRET_DELETE_TAG_KEY` (default: `ld-status`) - Tag key for deleted secrets
- `SECRET_DELETE_TAG_VALUE` (default: `deleted`) - Tag value for deleted secrets
- `ADD_DELETE_TIMESTAMP` (optional) - Set to `1` to add deletion timestamp

**Requirements**: AWS CLI configured with appropriate permissions

### write-gcp-secrets.sh
**Purpose**: Stores LaunchDarkly SDK credentials in Google Cloud Secret Manager  
**Method**: Google Cloud CLI (`gcloud secrets`)  
**Configuration**:
- `GOOGLE_APPLICATION_CREDENTIALS` or `gcloud auth` - Authentication method
- `PROJECT_ID` - GCP project ID (defaults to gcloud config project)
- `SECRET_PREFIX` (default: `launchdarkly-sdk`) - Prefix for secret names
- `ADD_DELETE_TIMESTAMP` (optional) - Set to `1` to add deletion timestamp

**Requirements**: Google Cloud CLI configured with Secret Manager permissions

### write-k8s-secrets.sh
**Purpose**: Stores LaunchDarkly SDK credentials as Kubernetes secrets  
**Method**: Kubernetes CLI (`kubectl`)  
**Configuration**:
- `KUBECONFIG` or kubectl context configured
- `NAMESPACE` (default: `default`) - Kubernetes namespace
- `SECRET_PREFIX` (default: `launchdarkly-sdk`) - Prefix for secret names
- `CLUSTER_NAME` (optional) - Cluster name for secret naming
- `ADD_DELETE_TIMESTAMP` (optional) - Set to `1` to add deletion timestamp

**Requirements**: kubectl configured with appropriate cluster access

### write-ld-relay-config.sh
**Purpose**: Generates LaunchDarkly Relay configuration file  
**Method**: File output (stdout or file)  
**Configuration**:
- `LDAC_LDR_CONFIG_FILE` (optional) - Output file path (defaults to stdout)
- `ENV_DATASTORE_PREFIX` (optional) - Datastore prefix for multi-environment support
- `ENV_DATASTORE_TABLE_NAME` (optional) - DynamoDB table name for multi-environment support

**Note**: Supports `$CID` replacement in prefix and table name with client ID

### write-ld-relay-env.sh
**Purpose**: Generates environment variables for LaunchDarkly Relay  
**Method**: File output (stdout or file)  
**Configuration**:
- `LDAC_LDR_ENV_FILE` (optional) - Output file path (defaults to stdout)
- `ENV_DATASTORE_PREFIX` (optional) - Datastore prefix for multi-environment support
- `ENV_DATASTORE_TABLE_NAME` (optional) - DynamoDB table name for multi-environment support

**Note**: Supports `$CID` replacement in prefix and table name with client ID


### write-vault-kv.sh
**Purpose**: Stores LaunchDarkly SDK credentials in HashiCorp Vault KV store  
**Method**: Vault CLI (`vault kv`)  
**Configuration**:
- `VAULT_ADDR` - Vault server address (required)
- `VAULT_TOKEN` or `VAULT_NAMESPACE` with auth method - Authentication
- `SECRET_PREFIX` (default: `launchdarkly-sdk`) - Prefix for secret paths
- `VAULT_MOUNT_PATH` (default: `secret`) - KV secrets engine mount path
- `VAULT_KV_VERSION` (default: `2`) - KV version (1 or 2)
- `ADD_DELETE_TIMESTAMP` (optional) - Set to `1` to add deletion timestamp

**Requirements**: Vault CLI configured with appropriate permissions
