use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Top-level config file (no secrets). Stored as config.toml.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
}

/// Per-provider config section.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub default_account: Option<String>,
    #[serde(default)]
    pub accounts: HashMap<String, Account>,
}

/// Metadata for a single account (no secrets).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub display_name: String,
    pub provider_id: String,
    pub user_id: String,
    pub created_at: u64,
}

/// Top-level credentials file. Stored as credentials.json with 0600 perms.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CredentialsFile {
    #[serde(default)]
    pub credentials: HashMap<String, StoredCredential>,
}

/// A single stored credential keyed as "provider/label".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCredential {
    pub fields: HashMap<String, String>,
    pub created_at: u64,
    pub last_validated: Option<u64>,
}

/// Result returned from a provider's authenticate method.
pub struct AuthResult {
    pub label: String,
    pub display_name: String,
    pub provider_id: String,
    pub user_id: String,
    pub credential: StoredCredential,
}
