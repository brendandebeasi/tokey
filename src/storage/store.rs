use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use super::types::*;

const CONFIG_FILE: &str = "config.toml";
const CREDENTIALS_FILE: &str = "credentials.json";

pub struct CredentialStore {
    config_dir: PathBuf,
    config_path: PathBuf,
    credentials_path: PathBuf,
}

impl CredentialStore {
    pub fn new() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .context("Could not determine config directory")?
            .join("tokey");

        fs::create_dir_all(&config_dir)?;

        Ok(Self {
            config_path: config_dir.join(CONFIG_FILE),
            credentials_path: config_dir.join(CREDENTIALS_FILE),
            config_dir,
        })
    }

    pub fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    // -- Config (no secrets) --------------------------------------------------

    pub fn load_config(&self) -> Result<Config> {
        if !self.config_path.exists() {
            return Ok(Config::default());
        }
        let contents = fs::read_to_string(&self.config_path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    }

    pub fn save_config(&self, config: &Config) -> Result<()> {
        let contents = toml::to_string_pretty(config)?;
        fs::write(&self.config_path, contents)?;
        Ok(())
    }

    // -- Credentials (secrets, 0600) ------------------------------------------

    pub fn load_credentials(&self) -> Result<CredentialsFile> {
        if !self.credentials_path.exists() {
            return Ok(CredentialsFile::default());
        }
        let contents = fs::read_to_string(&self.credentials_path)?;
        let creds: CredentialsFile = serde_json::from_str(&contents)?;
        Ok(creds)
    }

    pub fn save_credentials(&self, creds: &CredentialsFile) -> Result<()> {
        let contents = serde_json::to_string_pretty(creds)?;
        fs::write(&self.credentials_path, &contents)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.credentials_path, fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }

    // -- Credential key helper ------------------------------------------------

    fn cred_key(provider: &str, label: &str) -> String {
        format!("{}/{}", provider, label)
    }

    // -- High-level operations ------------------------------------------------

    pub fn save_account(
        &self,
        provider: &str,
        label: &str,
        account: Account,
        credential: StoredCredential,
    ) -> Result<()> {
        let mut config = self.load_config()?;
        let provider_config = config
            .providers
            .entry(provider.to_string())
            .or_default();

        provider_config
            .accounts
            .insert(label.to_string(), account);

        // Set as default if it's the first account
        if provider_config.default_account.is_none() {
            provider_config.default_account = Some(label.to_string());
        }

        self.save_config(&config)?;

        let mut creds = self.load_credentials()?;
        creds
            .credentials
            .insert(Self::cred_key(provider, label), credential);
        self.save_credentials(&creds)?;

        Ok(())
    }

    pub fn get_credential(&self, provider: &str, label: &str) -> Result<StoredCredential> {
        let creds = self.load_credentials()?;
        creds
            .credentials
            .get(&Self::cred_key(provider, label))
            .cloned()
            .context(format!("No credentials found for {}/{}", provider, label))
    }

    pub fn update_credential(
        &self,
        provider: &str,
        label: &str,
        credential: StoredCredential,
    ) -> Result<()> {
        let mut creds = self.load_credentials()?;
        creds
            .credentials
            .insert(Self::cred_key(provider, label), credential);
        self.save_credentials(&creds)?;
        Ok(())
    }

    pub fn mark_validated(&self, provider: &str, label: &str) -> Result<()> {
        let mut creds = self.load_credentials()?;
        if let Some(cred) = creds.credentials.get_mut(&Self::cred_key(provider, label)) {
            cred.last_validated = Some(Self::now());
            self.save_credentials(&creds)?;
        }
        Ok(())
    }

    pub fn remove_account(&self, provider: &str, label: &str) -> Result<()> {
        let mut config = self.load_config()?;
        if let Some(provider_config) = config.providers.get_mut(provider) {
            provider_config.accounts.remove(label);

            if provider_config.default_account.as_deref() == Some(label) {
                provider_config.default_account =
                    provider_config.accounts.keys().next().cloned();
            }

            // Remove provider entry if no accounts left
            if provider_config.accounts.is_empty() {
                config.providers.remove(provider);
            }
        }
        self.save_config(&config)?;

        let mut creds = self.load_credentials()?;
        creds.credentials.remove(&Self::cred_key(provider, label));
        self.save_credentials(&creds)?;

        // Remove chrome profile dir if it exists
        let profile_dir = self.chrome_profile_dir(provider, label);
        if profile_dir.exists() {
            let _ = fs::remove_dir_all(&profile_dir);
        }

        Ok(())
    }

    pub fn set_default(&self, provider: &str, label: &str) -> Result<()> {
        let mut config = self.load_config()?;
        let provider_config = config
            .providers
            .get_mut(provider)
            .context(format!("Provider '{}' not found", provider))?;

        if !provider_config.accounts.contains_key(label) {
            anyhow::bail!("Account '{}' not found under provider '{}'", label, provider);
        }

        provider_config.default_account = Some(label.to_string());
        self.save_config(&config)?;
        Ok(())
    }

    pub fn resolve_account(&self, provider: &str, account: Option<&str>) -> Result<String> {
        let config = self.load_config()?;
        let provider_config = config
            .providers
            .get(provider)
            .context(format!("Provider '{}' not found", provider))?;

        match account {
            Some(label) => {
                if provider_config.accounts.contains_key(label) {
                    Ok(label.to_string())
                } else {
                    anyhow::bail!("Account '{}' not found under provider '{}'", label, provider)
                }
            }
            None => provider_config
                .default_account
                .clone()
                .context(format!("No default account set for provider '{}'", provider)),
        }
    }

    pub fn is_expired(&self, provider: &str, label: &str, max_age_days: u64) -> Result<bool> {
        let cred = self.get_credential(provider, label)?;
        let age = Self::now() - cred.created_at;
        Ok(age > max_age_days * 24 * 60 * 60)
    }

    // -- Chrome profile paths -------------------------------------------------

    pub fn chrome_profile_dir(&self, provider: &str, label: &str) -> PathBuf {
        self.config_dir
            .join("chrome-profiles")
            .join(provider)
            .join(label)
    }

    pub fn temp_chrome_profile_dir(&self) -> PathBuf {
        self.config_dir.join("chrome-profile-tmp")
    }
}
