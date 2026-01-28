# tokey -- Agent Reference

## What This Is

tokey is a CLI credential manager for apps that don't support SSO/OAuth natively.
It extracts web session credentials via Chrome automation, stores them locally,
and refreshes them automatically. Consumers (other CLI tools, scripts) call
`tokey get <provider> [account]` to get fresh credentials on stdout.

Supported providers:
- **Slack** -- xoxc + d cookie via Chrome session extraction
- **Google** -- OAuth 2.0 access + refresh tokens (Gmail, Calendar)

## Build Commands

```bash
cargo build              # debug build
cargo build --release    # release build
cargo check              # type check only
cargo clippy             # lint
cargo fmt                # format
cargo test               # run tests
```

The binary name is `tokey`. There are no secondary binaries.

## Architecture

```
src/
  main.rs                # clap CLI entrypoint, command dispatch
  lib.rs                 # pub mod declarations
  cli/
    mod.rs               # re-exports
    commands.rs           # handler fn per CLI command + daemon management
  provider/
    mod.rs               # Provider trait, get_provider() registry
    slack.rs             # SlackProvider -- Chrome session extraction
    google.rs            # GoogleProvider -- OAuth 2.0 with refresh tokens
  auth/
    mod.rs               # re-exports
    chrome_auth.rs       # Chrome automation: extract tokens from localStorage
    browser_auth.rs      # HTML-based manual extraction (alternative flow)
    google_oauth.rs      # Google OAuth 2.0 flow with PKCE
    oauth.rs             # Slack OAuth 2.0 flow (kept, not primary path)
    pkce.rs              # PKCE challenge generation
  storage/
    mod.rs               # re-exports
    store.rs             # CredentialStore -- reads/writes config + credentials
    types.rs             # Config, Account, StoredCredential, AuthResult structs
```

### Key Abstractions

**Provider trait** (`provider/mod.rs`):
```rust
pub trait Provider {
    fn name(&self) -> &str;
    fn display_name(&self) -> &str;
    fn credential_fields(&self) -> &[&str];
    fn max_credential_age_days(&self) -> u64;
    fn authenticate(&self, store: &CredentialStore, label: &str) -> Result<AuthResult>;
    fn refresh(&self, store: &CredentialStore, label: &str) -> Result<StoredCredential>;
    fn validate(&self, credential: &StoredCredential) -> Result<bool>;
}
```

New providers implement this trait. `get_provider(name)` returns the right impl.

**CredentialStore** (`storage/store.rs`):
- Manages `~/.config/tokey/config.toml` (account metadata, no secrets)
- Manages `~/.config/tokey/credentials.json` (secrets, 0600 perms)
- Credential keys are `"provider/label"` (e.g. `"slack/work"`)
- Chrome profiles live at `~/.config/tokey/chrome-profiles/{provider}/{label}/`

**SlackProvider** (`provider/slack.rs`):
- `authenticate()` -- visible Chrome, user logs in interactively
- `refresh()` -- headless Chrome, reuses existing profile/session cookie
- `validate()` -- calls `slack.com/api/auth.test`

**GoogleProvider** (`provider/google.rs`):
- `authenticate()` -- opens browser to Google OAuth consent, localhost callback
- `refresh()` -- uses refresh_token to get new access_token (no browser)
- `validate()` -- calls userinfo endpoint to verify token
- Uses Thunderbird's public OAuth credentials (client_id + client_secret)
- Access tokens expire in 1 hour; refresh tokens are long-lived

### Data Flow

```
tokey add slack --label work
  -> SlackProvider::authenticate()
    -> chrome_auth::extract_credentials_with_chrome(profile_dir, existing=false, headless=false)
    -> Chrome opens visibly, user logs in
    -> JS extracts xoxc token from localStorage + d cookie
    -> Returns SlackCredentials
  -> CredentialStore::save_account() writes config.toml + credentials.json

tokey get slack work
  -> check is_expired (>30 days?)
  -> if expired: SlackProvider::refresh() (headless Chrome)
  -> CredentialStore::get_credential()
  -> print JSON to stdout

tokey add google --label personal
  -> GoogleProvider::authenticate()
    -> google_oauth::authenticate(scopes)
    -> Browser opens to Google OAuth consent
    -> User authorizes, redirected to localhost:8484/callback
    -> Exchange auth code for access_token + refresh_token
  -> CredentialStore::save_account() writes config.toml + credentials.json

tokey get google personal
  -> check expires_at (access token TTL)
  -> if expired: GoogleProvider::refresh() (uses refresh_token, no browser)
  -> CredentialStore::get_credential()
  -> print JSON to stdout

tokey refresh --all
  -> iterate all providers/accounts from config.toml
  -> for each: Provider::refresh() (headless)
  -> update credentials.json

tokey daemon install
  -> write ~/Library/LaunchAgents/dev.tokey.refresh.plist
  -> launchctl load
  -> launchd runs `tokey refresh --all` every N hours
```

### stdout vs stderr

All commands write status/progress to stderr. Only `get` writes to stdout,
and only the credential data (JSON or raw field value). This allows clean
shell piping: `export TOKEN=$(tokey get slack -f token)`.

## Code Style

- **Imports**: std first, external crates, then crate-local. Blank lines between groups.
- **Error handling**: `anyhow::Result` everywhere. Use `.context()` for error messages.
  `anyhow::bail!()` for early returns.
- **Naming**: snake_case for modules/functions, PascalCase for types, SCREAMING_SNAKE_CASE
  for constants.
- **No emojis in output**. Use plain text for all user-facing messages.
- **eprintln for status**, println for data output.

## Storage Files

```
~/.config/tokey/
  config.toml             # [providers.slack.accounts.work] metadata
  credentials.json        # {"credentials": {"slack/work": {fields, timestamps}}}
  daemon.log              # launchd agent output
  chrome-profiles/
    slack/work/            # persistent Chrome profile (has session cookies)
    slack/personal/        # separate profile per account
```

## Daemon

macOS launchd agent at `~/Library/LaunchAgents/dev.tokey.refresh.plist`.
Runs `tokey refresh --all` on a configurable interval (default 12h) and at
login. Logs to `~/.config/tokey/daemon.log`.

## Adding a New Provider

1. Create `src/provider/<name>.rs`
2. Implement the `Provider` trait
3. Add to the match in `get_provider()` in `provider/mod.rs`
4. Add to `all_provider_names()`

The provider decides how to authenticate and refresh. It can use the auth
modules (chrome_auth, browser_auth, oauth) or implement its own flow.
