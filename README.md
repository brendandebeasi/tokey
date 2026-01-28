# tokey

CLI credential manager for apps without SSO. Extracts, stores, and refreshes
web session credentials so other tools can consume them.

Supported providers:
- **Slack** -- xoxc + d cookie tokens via Chrome session extraction
- **Google** -- OAuth 2.0 for Gmail, Calendar (uses Thunderbird's public OAuth credentials)

## Quick Start

```bash
# Build
cargo build --release
cp target/release/tokey /usr/local/bin/

# Add a Slack account (opens Chrome, log in normally)
tokey add slack --label work

# Add a Google account (opens browser for OAuth consent)
tokey add google --label personal

# Get credentials as JSON
tokey get slack work
tokey get google personal

# Get just a specific field
tokey get slack work -f token
tokey get google personal -f access_token

# Check credential health
tokey status

# Set up automatic refresh daemon
tokey daemon install
```

## How It Works

### Slack

1. `tokey add slack` launches a real Chrome window pointed at `app.slack.com`
2. You log in normally -- SSO, password, whatever your workspace uses
3. tokey extracts `xoxc-*` tokens from localStorage and the `d=xoxd-*` session
   cookie from the browser
4. Credentials are saved to `~/.config/tokey/credentials.json` (0600 permissions)
5. The Chrome profile is persisted per-account at
   `~/.config/tokey/chrome-profiles/slack/<label>/` so future refreshes reuse
   the existing session

Slack refreshes run headless -- no browser window appears.

### Google

1. `tokey add google` opens your browser to Google's OAuth consent screen
2. You authorize access to Gmail and Calendar
3. tokey receives an authorization code and exchanges it for access + refresh tokens
4. Tokens are saved to `~/.config/tokey/credentials.json`
5. Access tokens expire hourly but are auto-refreshed using the refresh token

Google refresh happens via API (no browser needed) -- the refresh token is long-lived.

**Note:** tokey uses Thunderbird's public OAuth credentials. If you want to use
your own, you can create a Google Cloud project and set up OAuth credentials.

The daemon runs `tokey refresh --all` periodically to keep tokens fresh.

## Commands

```
tokey list [provider]                      # list providers and accounts
tokey get <provider> [account] [-f field]  # get creds (JSON to stdout)
tokey add <provider> [--label name]        # add account via browser
tokey refresh <provider> [account]         # force credential renewal
tokey refresh --all                        # refresh every account
tokey remove <provider> <account>          # delete account + creds
tokey status [provider] [account]          # credential health overview
tokey default <provider> <account>         # set default account
tokey daemon install [--interval 12]       # install launchd refresh agent
tokey daemon uninstall                     # remove launchd agent
tokey daemon status                        # check daemon state + recent logs
```

### Output conventions

- `get` writes credential JSON (or raw field value with `-f`) to **stdout**
- Everything else (status messages, progress, errors) goes to **stderr**
- This makes shell piping clean:

```bash
export SLACK_TOKEN=$(tokey get slack -f token)
export SLACK_COOKIE=$(tokey get slack -f cookie)

# Google access token for IMAP/API
export GOOGLE_ACCESS_TOKEN=$(tokey get google -f access_token)
```

### Auto-refresh

`get` checks credential freshness before returning:
- **Slack**: If credentials are older than 30 days, runs headless Chrome refresh
- **Google**: If access token is expired, uses refresh token to get a new one

If refresh fails, existing credentials are returned with a warning to stderr.

## Daemon Setup

The daemon uses macOS launchd to periodically refresh all credentials in the
background. No long-running process -- launchd wakes tokey on schedule.

```bash
# Install with default 12-hour interval
tokey daemon install

# Install with custom interval (e.g. every 6 hours)
tokey daemon install --interval 6

# Check if daemon is running
tokey daemon status

# View daemon logs
tail -f ~/.config/tokey/daemon.log

# Remove daemon
tokey daemon uninstall
```

The daemon:
- Runs `tokey refresh --all` at the configured interval
- Also runs once immediately on install and on each login
- Logs to `~/.config/tokey/daemon.log`
- Uses headless Chrome (no visible window)
- If a session cookie has expired (e.g. password changed), the headless refresh
  fails silently -- run `tokey add` again to re-authenticate interactively

The launchd plist is installed at `~/Library/LaunchAgents/dev.tokey.refresh.plist`.

## Storage Layout

```
~/.config/tokey/
  config.toml                         # account metadata (no secrets)
  credentials.json                    # tokens + cookies (0600 perms)
  daemon.log                          # daemon output
  chrome-profiles/
    slack/
      work/                           # Chrome profile for slack/work
      personal/                       # Chrome profile for slack/personal
```

### config.toml

```toml
[providers.slack]
default_account = "work"

[providers.slack.accounts.work]
display_name = "Acme Corp"
provider_id = "T01SF67KPH8"
user_id = "U09HNU993LM"
created_at = 1706400000
```

### credentials.json

```json
{
  "credentials": {
    "slack/work": {
      "fields": {
        "token": "xoxc-...",
        "cookie": "d=xoxd-..."
      },
      "created_at": 1706400000,
      "last_validated": null
    }
  }
}
```

## Multi-Account Support

Each account gets its own Chrome profile directory, so sessions are fully
isolated. You can have multiple Slack workspaces with different SSO providers:

```bash
tokey add slack --label work       # Okta SSO
tokey add slack --label personal   # Google SSO
tokey add slack --label client     # Password login

tokey default slack work           # set default
tokey get slack                    # uses default (work)
tokey get slack personal           # explicit account
```

## Building

Requires Rust toolchain and Chrome/Chromium installed on the system.

```bash
cargo build --release
```

The binary is at `target/release/tokey`. Copy it somewhere on your PATH.

## Security Notes

- Credentials are stored in `~/.config/tokey/credentials.json` with 0600
  permissions (owner read/write only)
- Chrome profiles contain session cookies -- treat the
  `~/.config/tokey/chrome-profiles/` directory as sensitive
- The `xoxc-*` tokens are client session tokens, not OAuth bot tokens -- they
  have the same permissions as your browser session
- Headless refresh reuses the existing Chrome session cookie without opening a
  visible window
- No credentials are ever sent anywhere except to `*.slack.com`

## License

MIT
