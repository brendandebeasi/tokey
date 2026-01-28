use std::collections::HashMap;

use anyhow::Result;

use crate::auth::google_oauth::{self, ALL_SCOPES};
use crate::storage::{AuthResult, CredentialStore, StoredCredential};

use super::Provider;

pub struct GoogleProvider;

impl Provider for GoogleProvider {
    fn name(&self) -> &str {
        "google"
    }

    fn display_name(&self) -> &str {
        "Google"
    }

    fn credential_fields(&self) -> &[&str] {
        &["access_token", "refresh_token", "email", "scopes", "expires_at"]
    }

    fn max_credential_age_days(&self) -> u64 {
        // Google access tokens expire in 1 hour, but we have refresh tokens
        // So this is really "how often to proactively refresh" - not critical
        // since we check expires_at and refresh on-demand
        7
    }

    fn authenticate(&self, _store: &CredentialStore, label: &str) -> Result<AuthResult> {
        eprintln!("Starting Google authentication...");
        eprintln!("Requesting access to: Gmail, Calendar, Contacts, Tasks, Drive, YouTube, Photos");
        eprintln!();

        let creds = google_oauth::authenticate(ALL_SCOPES)?;

        eprintln!();
        eprintln!("Successfully authenticated as: {}", creds.email);

        let mut fields = HashMap::new();
        fields.insert("access_token".to_string(), creds.access_token);
        fields.insert("refresh_token".to_string(), creds.refresh_token);
        fields.insert("email".to_string(), creds.email.clone());
        fields.insert("scopes".to_string(), creds.scopes.join(" "));
        fields.insert("expires_at".to_string(), creds.expires_at.to_string());

        Ok(AuthResult {
            label: label.to_string(),
            credential: StoredCredential {
                fields,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                last_validated: None,
            },
            display_name: creds.email.clone(),
            provider_id: creds.email, // Use email as provider_id for Google
            user_id: String::new(),
        })
    }

    fn refresh(&self, store: &CredentialStore, label: &str) -> Result<StoredCredential> {
        let existing = store.get_credential("google", label)?;

        let refresh_token = existing
            .fields
            .get("refresh_token")
            .ok_or_else(|| anyhow::anyhow!("No refresh token found"))?;

        eprintln!("Refreshing Google access token...");

        let (new_access_token, new_expires_at) = google_oauth::refresh_token(refresh_token)?;

        let mut fields = existing.fields.clone();
        fields.insert("access_token".to_string(), new_access_token);
        fields.insert("expires_at".to_string(), new_expires_at.to_string());

        eprintln!("Token refreshed successfully");

        Ok(StoredCredential {
            fields,
            created_at: existing.created_at,
            last_validated: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            ),
        })
    }

    fn validate(&self, credential: &StoredCredential) -> Result<bool> {
        let access_token = credential
            .fields
            .get("access_token")
            .ok_or_else(|| anyhow::anyhow!("No access token found"))?;

        // Check if token is expired
        if let Some(expires_at_str) = credential.fields.get("expires_at") {
            let expires_at: u64 = expires_at_str.parse().unwrap_or(0);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs();

            if now >= expires_at {
                return Ok(false); // Token expired
            }
        }

        // Try to get user info to validate token actually works
        let client = reqwest::blocking::Client::new();
        let response = client
            .get("https://www.googleapis.com/oauth2/v2/userinfo")
            .bearer_auth(access_token)
            .send()?;

        Ok(response.status().is_success())
    }
}

/// Check if credentials need refresh (access token expired or expiring soon)
pub fn needs_refresh(credential: &StoredCredential) -> bool {
    if let Some(expires_at_str) = credential.fields.get("expires_at") {
        let expires_at: u64 = expires_at_str.parse().unwrap_or(0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Refresh if expired or expiring in the next 5 minutes
        now + 300 >= expires_at
    } else {
        true // No expires_at, assume needs refresh
    }
}
