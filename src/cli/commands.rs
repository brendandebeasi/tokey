use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result};

use crate::provider;
use crate::storage::{Account, CredentialStore};

const PLIST_LABEL: &str = "dev.tokey.refresh";
const PLIST_FILENAME: &str = "dev.tokey.refresh.plist";

pub fn cmd_list(provider_filter: Option<&str>) -> Result<()> {
    let store = CredentialStore::new()?;
    let config = store.load_config()?;

    if config.providers.is_empty() {
        eprintln!("No accounts configured. Run `tokey add <provider>` to get started.");
        return Ok(());
    }

    let providers: Vec<&str> = match provider_filter {
        Some(name) => {
            if !config.providers.contains_key(name) {
                eprintln!("No accounts for provider '{}'.", name);
                return Ok(());
            }
            vec![name]
        }
        None => {
            let mut keys: Vec<&str> = config.providers.keys().map(|s| s.as_str()).collect();
            keys.sort();
            keys
        }
    };

    for prov_name in providers {
        if let Some(prov_config) = config.providers.get(prov_name) {
            println!("{}:", prov_name);
            let default = prov_config.default_account.as_deref().unwrap_or("");
            let mut labels: Vec<&String> = prov_config.accounts.keys().collect();
            labels.sort();
            for label in labels {
                let acct = &prov_config.accounts[label];
                let marker = if label.as_str() == default { " *" } else { "" };
                println!("  {}{} ({})", label, marker, acct.display_name);
            }
        }
    }

    Ok(())
}

pub fn cmd_get(provider_name: &str, account: Option<&str>, field: Option<&str>) -> Result<()> {
    let store = CredentialStore::new()?;
    let prov = provider::get_provider(provider_name)?;
    let label = store.resolve_account(provider_name, account)?;

    // Check if credentials need refresh
    let needs_refresh = if provider_name == "google" {
        // For Google, check if access token is expired based on expires_at
        store
            .get_credential(provider_name, &label)
            .map(|cred| {
                crate::provider::google::needs_refresh(&cred)
            })
            .unwrap_or(false)
    } else {
        // For other providers, check credential age
        store
            .is_expired(provider_name, &label, prov.max_credential_age_days())
            .unwrap_or(false)
    };

    if needs_refresh {
        if provider_name == "google" {
            eprintln!("Access token for {}/{} expired -- refreshing...", provider_name, label);
        } else {
            eprintln!(
                "Credentials for {}/{} are older than {} days -- refreshing...",
                provider_name,
                label,
                prov.max_credential_age_days()
            );
        }
        match prov.refresh(&store, &label) {
            Ok(new_cred) => {
                store.update_credential(provider_name, &label, new_cred)?;
                eprintln!("Credentials refreshed.");
            }
            Err(e) => {
                eprintln!("Refresh failed ({}), using existing credentials.", e);
            }
        }
    }

    let cred = store.get_credential(provider_name, &label)?;

    match field {
        Some(f) => {
            let val = cred
                .fields
                .get(f)
                .ok_or_else(|| anyhow::anyhow!("Field '{}' not found", f))?;
            println!("{}", val);
        }
        None => {
            let json = serde_json::to_string_pretty(&cred.fields)?;
            println!("{}", json);
        }
    }

    Ok(())
}

pub fn cmd_add(provider_name: &str, label: Option<&str>) -> Result<()> {
    let store = CredentialStore::new()?;
    let prov = provider::get_provider(provider_name)?;

    let effective_label = label.unwrap_or("default");

    eprintln!(
        "Adding {} account '{}'...",
        prov.display_name(),
        effective_label
    );

    let result = prov.authenticate(&store, effective_label)?;

    let account = Account {
        display_name: result.display_name.clone(),
        provider_id: result.provider_id.clone(),
        user_id: result.user_id.clone(),
        created_at: CredentialStore::now(),
    };

    store.save_account(provider_name, effective_label, account, result.credential)?;

    eprintln!(
        "Account saved: {}/{} ({})",
        provider_name, effective_label, result.display_name
    );

    Ok(())
}

pub fn cmd_refresh(provider_name: &str, account: Option<&str>) -> Result<()> {
    let store = CredentialStore::new()?;
    let prov = provider::get_provider(provider_name)?;
    let label = store.resolve_account(provider_name, account)?;

    eprintln!("Refreshing {}/{}...", provider_name, label);

    let new_cred = prov.refresh(&store, &label)?;
    store.update_credential(provider_name, &label, new_cred)?;

    eprintln!("Credentials refreshed for {}/{}.", provider_name, label);
    Ok(())
}

pub fn cmd_remove(provider_name: &str, account: &str) -> Result<()> {
    let store = CredentialStore::new()?;

    // Verify it exists
    store.resolve_account(provider_name, Some(account))?;

    store.remove_account(provider_name, account)?;
    eprintln!("Removed {}/{}.", provider_name, account);
    Ok(())
}

pub fn cmd_status(provider_filter: Option<&str>, account_filter: Option<&str>) -> Result<()> {
    let store = CredentialStore::new()?;
    let config = store.load_config()?;

    if config.providers.is_empty() {
        eprintln!("No accounts configured.");
        return Ok(());
    }

    let providers: Vec<&str> = match provider_filter {
        Some(name) => vec![name],
        None => {
            let mut keys: Vec<&str> = config.providers.keys().map(|s| s.as_str()).collect();
            keys.sort();
            keys
        }
    };

    for prov_name in providers {
        let prov_config = match config.providers.get(prov_name) {
            Some(c) => c,
            None => {
                eprintln!("Provider '{}' not found.", prov_name);
                continue;
            }
        };

        let prov = provider::get_provider(prov_name)?;

        let filter_label;
        let labels: Vec<&String> = match account_filter {
            Some(a) => {
                if prov_config.accounts.contains_key(a) {
                    filter_label = a.to_string();
                    vec![&filter_label]
                } else {
                    eprintln!("Account '{}' not found under '{}'.", a, prov_name);
                    continue;
                }
            }
            None => {
                let mut l: Vec<&String> = prov_config.accounts.keys().collect();
                l.sort();
                l
            }
        };

        println!("{}:", prov_name);
        let default = prov_config.default_account.as_deref().unwrap_or("");

        for label in &labels {
            let acct = &prov_config.accounts[label.as_str()];
            let marker = if label.as_str() == default {
                " [default]"
            } else {
                ""
            };

            let cred = store.get_credential(prov_name, label);
            let age_str = match &cred {
                Ok(c) => {
                    let age_secs = CredentialStore::now() - c.created_at;
                    let days = age_secs / 86400;
                    let max = prov.max_credential_age_days();
                    if days > max {
                        format!("{} days old (EXPIRED, max {})", days, max)
                    } else {
                        format!("{} days old (max {})", days, max)
                    }
                }
                Err(_) => "no credentials".to_string(),
            };

            let validated_str = match &cred {
                Ok(c) => match c.last_validated {
                    Some(ts) => {
                        let ago = CredentialStore::now() - ts;
                        let hours = ago / 3600;
                        if hours < 1 {
                            "validated <1h ago".to_string()
                        } else {
                            format!("validated {}h ago", hours)
                        }
                    }
                    None => "never validated".to_string(),
                },
                Err(_) => String::new(),
            };

            println!(
                "  {}{} -- {} | {} | {}",
                label, marker, acct.display_name, age_str, validated_str
            );
        }
    }

    Ok(())
}

pub fn cmd_default(provider_name: &str, account: &str) -> Result<()> {
    let store = CredentialStore::new()?;
    store.set_default(provider_name, account)?;
    eprintln!(
        "Default account for '{}' set to '{}'.",
        provider_name, account
    );
    Ok(())
}

pub fn cmd_refresh_all() -> Result<()> {
    let store = CredentialStore::new()?;
    let config = store.load_config()?;

    if config.providers.is_empty() {
        eprintln!("No accounts configured.");
        return Ok(());
    }

    let mut success = 0u32;
    let mut failed = 0u32;

    for (prov_name, prov_config) in &config.providers {
        let prov = match provider::get_provider(prov_name) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[{}] unknown provider: {}", prov_name, e);
                failed += 1;
                continue;
            }
        };

        for label in prov_config.accounts.keys() {
            eprintln!("[{}/{}] refreshing...", prov_name, label);
            match prov.refresh(&store, label) {
                Ok(new_cred) => {
                    if let Err(e) = store.update_credential(prov_name, label, new_cred) {
                        eprintln!("[{}/{}] save failed: {}", prov_name, label, e);
                        failed += 1;
                    } else {
                        eprintln!("[{}/{}] ok", prov_name, label);
                        success += 1;
                    }
                }
                Err(e) => {
                    eprintln!("[{}/{}] failed: {}", prov_name, label, e);
                    failed += 1;
                }
            }
        }
    }

    eprintln!("Done. {} refreshed, {} failed.", success, failed);

    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

// -- Daemon management --------------------------------------------------------

fn plist_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not determine home directory")?;
    Ok(home.join("Library/LaunchAgents").join(PLIST_FILENAME))
}

fn tokey_binary_path() -> Result<PathBuf> {
    std::env::current_exe().context("Could not determine tokey binary path")
}

fn log_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .context("Could not determine config directory")?
        .join("tokey");
    fs::create_dir_all(&config_dir)?;
    Ok(config_dir.join("daemon.log"))
}

pub fn cmd_daemon_install(interval_hours: u64) -> Result<()> {
    let plist = plist_path()?;
    let binary = tokey_binary_path()?;
    let log = log_path()?;
    let interval_secs = interval_hours * 3600;

    let plist_content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{binary}</string>
        <string>refresh</string>
        <string>--all</string>
    </array>
    <key>StartInterval</key>
    <integer>{interval}</integer>
    <key>RunAtLoad</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{log}</string>
    <key>StandardErrorPath</key>
    <string>{log}</string>
    <key>ProcessType</key>
    <string>Background</string>
</dict>
</plist>
"#,
        label = PLIST_LABEL,
        binary = binary.display(),
        interval = interval_secs,
        log = log.display(),
    );

    // Unload existing if present
    if plist.exists() {
        let _ = Command::new("launchctl")
            .args(["unload", &plist.to_string_lossy()])
            .output();
    }

    // Ensure LaunchAgents dir exists
    if let Some(parent) = plist.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(&plist, &plist_content)?;

    Command::new("launchctl")
        .args(["load", &plist.to_string_lossy()])
        .output()
        .context("Failed to run launchctl load")?;

    eprintln!("Daemon installed.");
    eprintln!("  Plist:    {}", plist.display());
    eprintln!("  Binary:   {}", binary.display());
    eprintln!("  Interval: every {} hours", interval_hours);
    eprintln!("  Log:      {}", log.display());
    eprintln!("  Runs at load: yes");
    eprintln!("");
    eprintln!("The daemon will run `tokey refresh --all` every {} hours", interval_hours);
    eprintln!("and once immediately on install/login.");
    eprintln!("");
    eprintln!("Check status:  tokey daemon status");
    eprintln!("View logs:     tail -f {}", log.display());
    eprintln!("Uninstall:     tokey daemon uninstall");

    Ok(())
}

pub fn cmd_daemon_uninstall() -> Result<()> {
    let plist = plist_path()?;

    if !plist.exists() {
        eprintln!("Daemon is not installed.");
        return Ok(());
    }

    Command::new("launchctl")
        .args(["unload", &plist.to_string_lossy()])
        .output()
        .context("Failed to run launchctl unload")?;

    fs::remove_file(&plist)?;

    eprintln!("Daemon uninstalled.");
    Ok(())
}

pub fn cmd_daemon_status() -> Result<()> {
    let plist = plist_path()?;

    if !plist.exists() {
        eprintln!("Daemon is not installed.");
        eprintln!("Run `tokey daemon install` to set up periodic credential refresh.");
        return Ok(());
    }

    eprintln!("Daemon is installed.");
    eprintln!("  Plist: {}", plist.display());

    let output = Command::new("launchctl")
        .args(["list", PLIST_LABEL])
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if out.status.success() {
                eprintln!("  Status: loaded");
                // Parse PID and last exit status from launchctl list output
                for line in stdout.lines() {
                    let line = line.trim();
                    if line.contains("PID") {
                        eprintln!("  {}", line);
                    }
                    if line.contains("LastExitStatus") {
                        eprintln!("  {}", line);
                    }
                }
            } else {
                eprintln!("  Status: not loaded (plist exists but not active)");
                eprintln!("  Run `tokey daemon install` to re-activate.");
            }
        }
        Err(_) => {
            eprintln!("  Status: could not query launchctl");
        }
    }

    let log = log_path()?;
    if log.exists() {
        eprintln!("  Log: {}", log.display());
        // Show last few lines
        let contents = fs::read_to_string(&log).unwrap_or_default();
        let lines: Vec<&str> = contents.lines().collect();
        let start = if lines.len() > 5 { lines.len() - 5 } else { 0 };
        if !lines.is_empty() {
            eprintln!("  Last log entries:");
            for line in &lines[start..] {
                eprintln!("    {}", line);
            }
        }
    }

    Ok(())
}
