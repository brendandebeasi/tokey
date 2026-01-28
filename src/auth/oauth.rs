use anyhow::{Context, Result};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::mpsc;
use tiny_http::{Response, Server};
use url::Url;

use super::pkce::PkceChallenge;

fn get_env_or_fail(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| {
        eprintln!("❌ Error: {} environment variable not set", key);
        eprintln!("💡 Set it with: export {}=your_value_here", key);
        std::process::exit(1);
    })
}
const REDIRECT_URI: &str = "http://localhost:8484/callback";
const AUTH_URL: &str = "https://slack.com/oauth/v2/authorize";
const TOKEN_URL: &str = "https://slack.com/api/oauth.v2.access";

const SCOPES: &[&str] = &[
    "channels:history",
    "channels:read",
    "chat:write",
    "groups:history",
    "groups:read",
    "im:history",
    "im:read",
    "im:write",
    "mpim:history",
    "mpim:read",
    "users:read",
    "team:read",
];

#[derive(Debug, Serialize, Deserialize)]
pub struct OAuthResult {
    pub access_token: String,
    pub team_id: String,
    pub team_name: String,
    pub user_id: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    ok: bool,
    access_token: Option<String>,
    team: Option<Team>,
    authed_user: Option<AuthedUser>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Team {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct AuthedUser {
    id: String,
    access_token: Option<String>,
}

pub fn start_oauth_flow() -> Result<OAuthResult> {
    let client_id = get_env_or_fail("SLACK_CLIENT_ID");
    let client_secret = get_env_or_fail("SLACK_CLIENT_SECRET");

    let pkce = PkceChallenge::generate();
    let state: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();

    let mut auth_url = Url::parse(AUTH_URL)?;
    auth_url
        .query_pairs_mut()
        .append_pair("client_id", &client_id)
        .append_pair("scope", &SCOPES.join(","))
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("state", &state)
        .append_pair("code_challenge", &pkce.challenge)
        .append_pair("code_challenge_method", "S256");

    eprintln!("Opening browser for Slack authentication...");
    eprintln!("If browser doesn't open, visit:\n{}", auth_url);

    open_browser(&auth_url.to_string())?;

    let (tx, rx) = mpsc::channel();
    let server = Server::http("127.0.0.1:8484")
        .map_err(|e| anyhow::anyhow!("Failed to start local callback server: {}", e))?;

    eprintln!("Waiting for authorization...");

    for request in server.incoming_requests() {
        let url_path = request.url();
        let url = format!("http://localhost{}", url_path);
        let parsed = Url::parse(&url)?;
        let params: HashMap<_, _> = parsed.query_pairs().collect();

        if let (Some(code), Some(recv_state)) = (params.get("code"), params.get("state")) {
            if recv_state.as_ref() != state {
                let response = Response::from_string("❌ State mismatch! Please try again.");
                let _ = request.respond(response);
                anyhow::bail!("OAuth state mismatch");
            }

            let response = Response::from_string(
                "<html><body><h1>✅ Authentication successful!</h1><p>You can close this window and return to your terminal.</p></body></html>"
            );
            let _ = request.respond(response);

            tx.send(code.to_string())?;
            break;
        } else if params.get("error").is_some() {
            let response = Response::from_string("❌ Authorization denied!");
            let _ = request.respond(response);
            anyhow::bail!("User denied authorization");
        }
    }

    let code = rx.recv()?;
    exchange_code_for_token(&code, &pkce.verifier, &client_id, &client_secret)
}

fn exchange_code_for_token(
    code: &str,
    verifier: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<OAuthResult> {
    let client = reqwest::blocking::Client::new();

    let mut params = HashMap::new();
    params.insert("client_id", client_id);
    params.insert("client_secret", client_secret);
    params.insert("code", code);
    params.insert("redirect_uri", REDIRECT_URI);
    params.insert("code_verifier", verifier);

    let response: TokenResponse = client.post(TOKEN_URL).form(&params).send()?.json()?;

    if !response.ok {
        anyhow::bail!(
            "Token exchange failed: {}",
            response.error.unwrap_or_default()
        );
    }

    let team = response.team.context("Missing team info")?;
    let authed_user = response.authed_user.context("Missing user info")?;
    let access_token = authed_user
        .access_token
        .or(response.access_token)
        .context("Missing access token")?;

    Ok(OAuthResult {
        access_token,
        team_id: team.id,
        team_name: team.name,
        user_id: authed_user.id,
    })
}

fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(url).spawn()?;

    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open").arg(url).spawn()?;

    #[cfg(target_os = "windows")]
    std::process::Command::new("cmd")
        .args(&["/C", "start", url])
        .spawn()?;

    Ok(())
}
