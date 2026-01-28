use std::collections::HashMap;

use anyhow::{Context, Result};

use crate::auth::chrome_auth;
use crate::storage::{AuthResult, CredentialStore, StoredCredential};

use super::Provider;

pub struct SlackProvider;

impl Provider for SlackProvider {
    fn name(&self) -> &str {
        "slack"
    }

    fn display_name(&self) -> &str {
        "Slack"
    }

    fn credential_fields(&self) -> &[&str] {
        &["token", "cookie"]
    }

    fn max_credential_age_days(&self) -> u64 {
        30
    }

    fn authenticate(&self, store: &CredentialStore, label: &str) -> Result<AuthResult> {
        let profile_dir = store.chrome_profile_dir("slack", label);
        let existing = profile_dir.exists();

        // Interactive login -- always visible so user can SSO/captcha
        let creds = chrome_auth::extract_credentials_with_chrome(profile_dir, existing, false)?;

        let mut fields = HashMap::new();
        fields.insert("token".to_string(), creds.token);
        fields.insert("cookie".to_string(), creds.cookie);

        let now = CredentialStore::now();

        Ok(AuthResult {
            label: label.to_string(),
            display_name: creds.team_name,
            provider_id: creds.team_id,
            user_id: creds.user_id,
            credential: StoredCredential {
                fields,
                created_at: now,
                last_validated: None,
            },
        })
    }

    fn refresh(&self, store: &CredentialStore, label: &str) -> Result<StoredCredential> {
        let profile_dir = store.chrome_profile_dir("slack", label);
        // Headless -- reuses existing Chrome profile with session cookies
        let creds = chrome_auth::extract_credentials_with_chrome(profile_dir, true, true)?;

        let mut fields = HashMap::new();
        fields.insert("token".to_string(), creds.token);
        fields.insert("cookie".to_string(), creds.cookie);

        let now = CredentialStore::now();

        Ok(StoredCredential {
            fields,
            created_at: now,
            last_validated: None,
        })
    }

    fn validate(&self, credential: &StoredCredential) -> Result<bool> {
        let token = credential
            .fields
            .get("token")
            .context("Missing 'token' field")?;
        let cookie = credential
            .fields
            .get("cookie")
            .context("Missing 'cookie' field")?;

        let client = reqwest::blocking::Client::new();
        let resp: serde_json::Value = client
            .post("https://slack.com/api/auth.test")
            .header("Authorization", format!("Bearer {}", token))
            .header("Cookie", cookie.as_str())
            .send()?
            .json()?;

        Ok(resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
    }
}
