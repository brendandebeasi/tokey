pub mod slack;

use anyhow::Result;

use crate::storage::{AuthResult, CredentialStore, StoredCredential};

pub trait Provider {
    fn name(&self) -> &str;
    fn display_name(&self) -> &str;
    fn credential_fields(&self) -> &[&str];
    fn max_credential_age_days(&self) -> u64;
    fn authenticate(&self, store: &CredentialStore, label: &str) -> Result<AuthResult>;
    fn refresh(&self, store: &CredentialStore, label: &str) -> Result<StoredCredential>;
    fn validate(&self, credential: &StoredCredential) -> Result<bool>;
}

pub fn get_provider(name: &str) -> Result<Box<dyn Provider>> {
    match name {
        "slack" => Ok(Box::new(slack::SlackProvider)),
        _ => anyhow::bail!("Unknown provider: '{}'. Available: slack", name),
    }
}

pub fn all_provider_names() -> &'static [&'static str] {
    &["slack"]
}
