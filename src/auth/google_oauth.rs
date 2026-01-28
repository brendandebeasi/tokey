use anyhow::{Context, Result};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::mpsc;
use tiny_http::{Response, Server};
use url::Url;

use super::pkce::PkceChallenge;

// Thunderbird's public OAuth credentials for Google
// Source: https://hg.mozilla.org/comm-central/file/tip/mailnews/base/src/OAuth2Providers.jsm
const GOOGLE_CLIENT_ID: &str = "406964657835-aq8lmia8j95dhl1a2bvharmfk3t1hgof.apps.googleusercontent.com";
const GOOGLE_CLIENT_SECRET: &str = "kSmqreRr0qwBWJgbf5Y-PjSU";

const REDIRECT_URI: &str = "http://localhost:8484/callback";
const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

// Available scopes - user chooses which ones they need
pub const SCOPE_GMAIL: &str = "https://mail.google.com/";
pub const SCOPE_CALENDAR: &str = "https://www.googleapis.com/auth/calendar";
pub const SCOPE_CONTACTS: &str = "https://www.googleapis.com/auth/contacts";
pub const SCOPE_USERINFO: &str = "https://www.googleapis.com/auth/userinfo.email";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleCredentials {
    pub access_token: String,
    pub refresh_token: String,
    pub email: String,
    pub scopes: Vec<String>,
    pub expires_at: u64,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
    scope: Option<String>,
    token_type: String,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserInfo {
    email: String,
}

pub fn authenticate(scopes: &[&str]) -> Result<GoogleCredentials> {
    let pkce = PkceChallenge::generate();
    let state: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();

    let scope_str = scopes.join(" ");

    let mut auth_url = Url::parse(AUTH_URL)?;
    auth_url
        .query_pairs_mut()
        .append_pair("client_id", GOOGLE_CLIENT_ID)
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("response_type", "code")
        .append_pair("scope", &format!("{} {}", scope_str, SCOPE_USERINFO))
        .append_pair("state", &state)
        .append_pair("code_challenge", &pkce.challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent"); // Force consent to get refresh_token

    eprintln!("Opening browser for Google authentication...");
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
                let response = Response::from_string("State mismatch! Please try again.");
                let _ = request.respond(response);
                anyhow::bail!("OAuth state mismatch");
            }

            let response = Response::from_string(
                "<html><body><h1>Authentication successful!</h1><p>You can close this window and return to your terminal.</p></body></html>"
            );
            let _ = request.respond(response);

            tx.send(code.to_string())?;
            break;
        } else if let Some(error) = params.get("error") {
            let desc = params.get("error_description").map(|s| s.to_string()).unwrap_or_default();
            let response = Response::from_string(format!("Authorization failed: {} - {}", error, desc));
            let _ = request.respond(response);
            anyhow::bail!("Authorization failed: {} - {}", error, desc);
        }
    }

    let code = rx.recv()?;
    let tokens = exchange_code_for_token(&code, &pkce.verifier)?;

    // Get user email
    let email = get_user_email(&tokens.access_token)?;

    let expires_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        + tokens.expires_in;

    Ok(GoogleCredentials {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token.context("No refresh token received. Try revoking app access at https://myaccount.google.com/permissions and re-authenticating.")?,
        email,
        scopes: scopes.iter().map(|s| s.to_string()).collect(),
        expires_at,
    })
}

pub fn refresh_token(refresh_token: &str) -> Result<(String, u64)> {
    let client = reqwest::blocking::Client::new();

    let mut params = HashMap::new();
    params.insert("client_id", GOOGLE_CLIENT_ID);
    params.insert("client_secret", GOOGLE_CLIENT_SECRET);
    params.insert("refresh_token", refresh_token);
    params.insert("grant_type", "refresh_token");

    let response: TokenResponse = client
        .post(TOKEN_URL)
        .form(&params)
        .send()?
        .json()?;

    if let Some(error) = response.error {
        let desc = response.error_description.unwrap_or_default();
        anyhow::bail!("Token refresh failed: {} - {}", error, desc);
    }

    let expires_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        + response.expires_in;

    Ok((response.access_token, expires_at))
}

fn exchange_code_for_token(code: &str, verifier: &str) -> Result<TokenResponse> {
    let client = reqwest::blocking::Client::new();

    let mut params = HashMap::new();
    params.insert("client_id", GOOGLE_CLIENT_ID);
    params.insert("client_secret", GOOGLE_CLIENT_SECRET);
    params.insert("code", code);
    params.insert("redirect_uri", REDIRECT_URI);
    params.insert("code_verifier", verifier);
    params.insert("grant_type", "authorization_code");

    let response: TokenResponse = client
        .post(TOKEN_URL)
        .form(&params)
        .send()?
        .json()?;

    if let Some(error) = response.error {
        let desc = response.error_description.unwrap_or_default();
        anyhow::bail!("Token exchange failed: {} - {}", error, desc);
    }

    Ok(response)
}

fn get_user_email(access_token: &str) -> Result<String> {
    let client = reqwest::blocking::Client::new();

    let response: UserInfo = client
        .get("https://www.googleapis.com/oauth2/v2/userinfo")
        .bearer_auth(access_token)
        .send()?
        .json()?;

    Ok(response.email)
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
