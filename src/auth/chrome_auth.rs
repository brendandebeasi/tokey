use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use headless_chrome::{Browser, LaunchOptions};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SlackCredentials {
    pub token: String,
    pub cookie: String,
    pub team_id: String,
    pub team_name: String,
    pub user_id: String,
}

pub fn extract_credentials_with_chrome(
    profile_dir: PathBuf,
    existing_session: bool,
    headless: bool,
) -> Result<SlackCredentials> {
    if headless {
        eprintln!("Refreshing credentials (headless)...");
    } else if existing_session {
        eprintln!("Restoring Chrome session to refresh credentials...");
        eprintln!("Your previous login should still be valid.");
    } else {
        eprintln!("Launching Chrome for Slack authentication...");
        eprintln!("A Chrome window will open -- log in to Slack normally (SSO supported).");
        eprintln!("Your login will be saved in a per-account Chrome profile.");
    }
    if !headless {
        eprintln!("Waiting for login...");
    }

    std::fs::create_dir_all(&profile_dir)?;

    let options = LaunchOptions {
        headless,
        window_size: Some((1200, 900)),
        user_data_dir: Some(profile_dir),
        ..Default::default()
    };

    let browser =
        Browser::new(options).context("Failed to launch Chrome. Is Chrome/Chromium installed?")?;

    let tab = browser.new_tab().context("Failed to create browser tab")?;

    tab.navigate_to("https://app.slack.com/client")
        .context("Failed to navigate to Slack")?;

    if existing_session {
        eprintln!("Checking if session is still valid...");
    } else {
        eprintln!("Waiting for you to log in to Slack...");
        eprintln!("(The window will close automatically after login)");
    }

    let mut check_count = 0;
    let max_attempts = if existing_session { 30 } else { 300 };

    loop {
        std::thread::sleep(Duration::from_secs(2));
        check_count += 1;

        if check_count > max_attempts {
            anyhow::bail!("Timeout waiting for login. Please try again.");
        }

        if !existing_session && check_count % 5 == 0 {
            eprintln!("  Still waiting... (checked {} times)", check_count);
        }

        let result = tab.evaluate(
            r#"
            (function() {
                try {
                    const localConfig = localStorage.getItem('localConfig_v2');
                    if (!localConfig) {
                        return { logged_in: false };
                    }

                    const config = JSON.parse(localConfig);
                    const teams = config.teams || {};
                    const teamIds = Object.keys(teams);
                    
                    if (teamIds.length === 0) {
                        return { logged_in: false };
                    }

                    const team = teams[teamIds[0]];
                    if (!team || !team.token || !team.token.startsWith('xoxc-')) {
                        return { logged_in: false };
                    }

                    function getCookie(name) {
                        const value = '; ' + document.cookie;
                        const parts = value.split('; ' + name + '=');
                        if (parts.length === 2) return parts.pop().split(';').shift();
                        return null;
                    }

                    const dCookie = getCookie('d');
                    if (!dCookie || !dCookie.startsWith('xoxd-')) {
                        return { logged_in: false };
                    }

                    return {
                        logged_in: true,
                        token: team.token,
                        cookie: 'd=' + dCookie,
                        team_id: team.id,
                        team_name: team.name,
                        user_id: team.user_id || 'unknown'
                    };
                } catch (e) {
                    return { logged_in: false, error: e.message };
                }
            })();
            "#,
            false,
        );

        if let Ok(eval_result) = result {
            if let Some(value) = eval_result.value {
                if let Some(obj) = value.as_object() {
                    if let Some(logged_in) = obj.get("logged_in") {
                        if logged_in.as_bool() == Some(true) {
                            let token = obj
                                .get("token")
                                .and_then(|v: &serde_json::Value| v.as_str())
                                .context("Failed to extract token")?
                                .to_string();

                            let cookie = obj
                                .get("cookie")
                                .and_then(|v: &serde_json::Value| v.as_str())
                                .context("Failed to extract cookie")?
                                .to_string();

                            let team_id = obj
                                .get("team_id")
                                .and_then(|v: &serde_json::Value| v.as_str())
                                .context("Failed to extract team_id")?
                                .to_string();

                            let team_name = obj
                                .get("team_name")
                                .and_then(|v: &serde_json::Value| v.as_str())
                                .context("Failed to extract team_name")?
                                .to_string();

                            let user_id = obj
                                .get("user_id")
                                .and_then(|v: &serde_json::Value| v.as_str())
                                .unwrap_or("unknown")
                                .to_string();

                            eprintln!("Credentials extracted successfully.");
                            eprintln!("Workspace: {}", team_name);

                            return Ok(SlackCredentials {
                                token,
                                cookie,
                                team_id,
                                team_name,
                                user_id,
                            });
                        }
                    }
                }
            }
        }
    }
}

pub fn extract_all_workspaces_with_chrome(profile_dir: PathBuf) -> Result<Vec<SlackCredentials>> {
    eprintln!("Launching Chrome to extract all workspaces...");

    std::fs::create_dir_all(&profile_dir)?;

    let options = LaunchOptions {
        headless: false,
        window_size: Some((1200, 900)),
        user_data_dir: Some(profile_dir),
        ..Default::default()
    };

    let browser = Browser::new(options)?;
    let tab = browser.new_tab()?;

    tab.navigate_to("https://app.slack.com/client")?;

    eprintln!("Waiting for page load...");
    std::thread::sleep(Duration::from_secs(3));

    let result = tab.evaluate(
        r#"
        (function() {
            try {
                const localConfig = localStorage.getItem('localConfig_v2');
                if (!localConfig) {
                    return { workspaces: [] };
                }

                const config = JSON.parse(localConfig);
                const teams = config.teams || {};
                const teamIds = Object.keys(teams);
                
                function getCookie(name) {
                    const value = '; ' + document.cookie;
                    const parts = value.split('; ' + name + '=');
                    if (parts.length === 2) return parts.pop().split(';').shift();
                    return null;
                }

                const dCookie = getCookie('d');
                const workspaces = teamIds.map(teamId => {
                    const team = teams[teamId];
                    return {
                        token: team.token,
                        cookie: 'd=' + dCookie,
                        team_id: team.id,
                        team_name: team.name,
                        user_id: team.user_id || 'unknown'
                    };
                });

                return { workspaces };
            } catch (e) {
                return { workspaces: [], error: e.message };
            }
        })();
        "#,
        false,
    )?;

    let mut all_workspaces = Vec::new();

    if let Some(value) = result.value {
        if let Some(obj) = value.as_object() {
            if let Some(workspaces_array) = obj.get("workspaces").and_then(|v| v.as_array()) {
                for workspace_value in workspaces_array {
                    if let Some(ws) = workspace_value.as_object() {
                        let token = ws.get("token").and_then(|v| v.as_str()).unwrap_or("");
                        let cookie = ws.get("cookie").and_then(|v| v.as_str()).unwrap_or("");
                        let team_id = ws.get("team_id").and_then(|v| v.as_str()).unwrap_or("");
                        let team_name = ws.get("team_name").and_then(|v| v.as_str()).unwrap_or("");
                        let user_id = ws
                            .get("user_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");

                        if !token.is_empty() && token.starts_with("xoxc-") {
                            all_workspaces.push(SlackCredentials {
                                token: token.to_string(),
                                cookie: cookie.to_string(),
                                team_id: team_id.to_string(),
                                team_name: team_name.to_string(),
                                user_id: user_id.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    eprintln!("Found {} workspace(s)", all_workspaces.len());
    for ws in &all_workspaces {
        eprintln!("  {}", ws.team_name);
    }

    Ok(all_workspaces)
}
