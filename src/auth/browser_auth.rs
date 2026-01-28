use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::mpsc;
use tiny_http::{Response, Server};

#[derive(Debug, Serialize, Deserialize)]
pub struct SlackCredentials {
    pub token: String,
    pub cookie: String,
    pub team_id: String,
    pub team_name: String,
    pub user_id: String,
}

const START_HTML: &str = r#"
<!DOCTYPE html>
<html>
<head>
    <title>tokey - Authentication</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            padding: 20px;
            max-width: 800px;
            margin: 0 auto;
            background: #f8f8f8;
        }
        .container {
            background: white;
            padding: 30px;
            border-radius: 8px;
            box-shadow: 0 2px 10px rgba(0,0,0,0.1);
        }
        .btn {
            display: inline-block;
            padding: 12px 24px;
            background: #611f69;
            color: white;
            text-decoration: none;
            border-radius: 4px;
            font-weight: bold;
            margin: 10px 5px;
            font-size: 16px;
            cursor: pointer;
            border: none;
        }
        .btn:hover {
            background: #4a154b;
        }
        .instructions {
            background: #f0f0f0;
            padding: 20px;
            border-radius: 4px;
            margin: 20px 0;
        }
        .step {
            margin: 15px 0;
            padding: 15px;
            background: white;
            border-left: 4px solid #611f69;
        }
        .code-box {
            background: #2d2d2d;
            color: #f8f8f8;
            padding: 15px;
            border-radius: 4px;
            font-family: monospace;
            font-size: 13px;
            overflow-x: auto;
            margin: 10px 0;
            cursor: pointer;
        }
        .code-box:hover {
            background: #3d3d3d;
        }
        .success {
            color: #2eb886;
            font-weight: bold;
        }
    </style>
</head>
<body>
    <div class="container">
        <h1>🔐 tokey - Browser Authentication</h1>
        <p>Extract your Slack credentials to use in the terminal client.</p>
        
        <div class="instructions">
            <h3>Step-by-Step Instructions:</h3>
            
            <div class="step">
                <strong>Step 1:</strong> Open Slack web app
                <br><br>
                <a href="https://app.slack.com/client" class="btn" target="_blank">Open Slack</a>
                <br><small>Opens in a new tab - log in if needed</small>
            </div>
            
            <div class="step">
                <strong>Step 2:</strong> Open Browser DevTools Console
                <ul>
                    <li><strong>Mac:</strong> Cmd + Option + J (Chrome) or Cmd + Option + C (Safari)</li>
                    <li><strong>Windows/Linux:</strong> Ctrl + Shift + J (Chrome) or F12</li>
                </ul>
            </div>
            
            <div class="step">
                <strong>Step 3:</strong> Copy and paste this code into the console:
                <div class="code-box" id="extractScript" onclick="copyScript()">
(function(){
function getCookie(name){const v='; '+document.cookie;const p=v.split('; '+name+'=');if(p.length===2)return p.pop().split(';').shift();return null;}
const d=getCookie('d');
if(!d||!d.startsWith('xoxd-')){alert('❌ Not logged in to Slack');return;}
const cfg=localStorage.getItem('localConfig_v2');
if(!cfg){alert('❌ Slack data not found');return;}
const c=JSON.parse(cfg);
const t=Object.values(c.teams||{})[0];
if(!t||!t.token){alert('❌ No workspace found');return;}
fetch('http://localhost:8484/credentials',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({token:t.token,cookie:'d='+d,team_id:t.id,team_name:t.name,user_id:t.user_id||'unknown'})}).then(()=>alert('✅ Success! Return to terminal.')).catch(e=>alert('❌ Failed: '+e.message));
})();
                </div>
                <small>Click the code box to copy, then paste into console and press Enter</small>
            </div>
            
            <div class="step">
                <strong>Step 4:</strong> Check for success
                <ul>
                    <li>You should see: <span class="success">✅ Success! Return to terminal.</span></li>
                    <li>Return to your terminal - credentials are saved!</li>
                </ul>
            </div>
        </div>
        
        <p><small><strong>Note:</strong> This extracts your own session credentials. They're stored securely in your OS keyring.</small></p>
    </div>
    
    <script>
        function copyScript() {
            const script = document.getElementById('extractScript').textContent.trim();
            navigator.clipboard.writeText(script).then(() => {
                alert('✅ Script copied! Paste it into Slack\'s DevTools console.');
            }).catch(() => {
                alert('Select the text and copy manually (Cmd+C or Ctrl+C)');
            });
        }
    </script>
</body>
</html>
"#;

const CALLBACK_HTML: &str = r#"
<!DOCTYPE html>
<html>
<head>
    <title>tokey - Extracting Credentials...</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            display: flex;
            justify-content: center;
            align-items: center;
            height: 100vh;
            margin: 0;
            background: #f8f8f8;
        }
        .container {
            text-align: center;
            background: white;
            padding: 40px;
            border-radius: 8px;
            box-shadow: 0 2px 10px rgba(0,0,0,0.1);
        }
        .spinner {
            border: 3px solid #f3f3f3;
            border-top: 3px solid #611f69;
            border-radius: 50%;
            width: 40px;
            height: 40px;
            animation: spin 1s linear infinite;
            margin: 20px auto;
        }
        @keyframes spin {
            0% { transform: rotate(0deg); }
            100% { transform: rotate(360deg); }
        }
        .success { color: #2eb886; }
        .error { color: #e01e5a; }
    </style>
</head>
<body>
    <div class="container">
        <h2>Extracting Slack Credentials...</h2>
        <div class="spinner"></div>
        <p id="status">Please wait...</p>
    </div>
    
    <script>
        const status = document.getElementById('status');
        
        function getCookie(name) {
            const value = `; ${document.cookie}`;
            const parts = value.split(`; ${name}=`);
            if (parts.length === 2) return parts.pop().split(';').shift();
            return null;
        }
        
        function extractCredentials() {
            try {
                const dCookie = getCookie('d');
                if (!dCookie || !dCookie.startsWith('xoxd-')) {
                    throw new Error('Session cookie not found. Please log in to Slack first.');
                }
                
                const localConfig = localStorage.getItem('localConfig_v2');
                if (!localConfig) {
                    throw new Error('Slack localStorage not found. Please log in to Slack first.');
                }
                
                const config = JSON.parse(localConfig);
                const teams = config.teams || {};
                const teamIds = Object.keys(teams);
                
                if (teamIds.length === 0) {
                    throw new Error('No teams found. Please log in to Slack first.');
                }
                
                const team = teams[teamIds[0]];
                const token = team.token;
                
                if (!token || !token.startsWith('xoxc-')) {
                    throw new Error('Invalid token format. Expected xoxc- prefix.');
                }
                
                const credentials = {
                    token: token,
                    cookie: `d=${dCookie}`,
                    team_id: team.id,
                    team_name: team.name,
                    user_id: team.user_id || 'unknown'
                };
                
                status.textContent = 'Credentials extracted! Sending to application...';
                status.className = 'success';
                
                fetch('http://localhost:8484/credentials', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify(credentials)
                }).then(() => {
                    status.textContent = '✅ Success! You can close this window.';
                    setTimeout(() => window.close(), 2000);
                }).catch(err => {
                    throw new Error('Failed to send credentials: ' + err.message);
                });
                
            } catch (err) {
                status.textContent = '❌ ' + err.message;
                status.className = 'error';
                console.error(err);
            }
        }
        
        setTimeout(extractCredentials, 1000);
    </script>
</body>
</html>
"#;

pub fn start_browser_auth() -> Result<SlackCredentials> {
    let (tx, rx) = mpsc::channel();

    let server = Server::http("127.0.0.1:8484")
        .map_err(|e| anyhow::anyhow!("Failed to start callback server: {}", e))?;

    eprintln!("Opening Slack in browser for authentication...");
    eprintln!("Steps:");
    eprintln!("  1. Browser will open to localhost (don't close it!)");
    eprintln!("  2. Click the link to open Slack");
    eprintln!("  3. Log in to Slack (SSO supported)");
    eprintln!("  4. Credentials will be extracted automatically");

    let callback_url = "http://localhost:8484/extract";
    let slack_url = format!(
        "https://app.slack.com/client?redirect_url={}",
        urlencoding::encode(callback_url)
    );

    open_browser("http://localhost:8484/start")?;

    eprintln!("Waiting for authentication...");

    for mut request in server.incoming_requests() {
        let url_path = request.url().to_string();

        if url_path == "/start" {
            let html = START_HTML.replace("SLACK_URL_PLACEHOLDER", &slack_url);
            let response = Response::from_string(html).with_header(
                tiny_http::Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"text/html; charset=utf-8"[..],
                )
                .unwrap(),
            );
            let _ = request.respond(response);
            continue;
        }

        if url_path == "/extract" {
            let response = Response::from_string(CALLBACK_HTML).with_header(
                tiny_http::Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"text/html; charset=utf-8"[..],
                )
                .unwrap(),
            );
            let _ = request.respond(response);
            continue;
        }

        if url_path == "/credentials" {
            let mut content = String::new();
            if let Err(e) = request.as_reader().read_to_string(&mut content) {
                eprintln!("Failed to read request body: {}", e);
                continue;
            }

            match serde_json::from_str::<SlackCredentials>(&content) {
                Ok(creds) => {
                    let response = Response::from_string("OK");
                    let _ = request.respond(response);
                    tx.send(creds).ok();
                    break;
                }
                Err(e) => {
                    eprintln!("Failed to parse credentials: {}", e);
                    let response =
                        Response::from_string(format!("Error: {}", e)).with_status_code(400);
                    let _ = request.respond(response);
                }
            }
        }
    }

    rx.recv()
        .context("Failed to receive credentials from browser")
}

fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let browsers = ["Safari", "Google Chrome", "Firefox", "Brave Browser"];
        let mut opened = false;

        for browser in &browsers {
            if std::process::Command::new("open")
                .arg("-a")
                .arg(browser)
                .arg(url)
                .spawn()
                .is_ok()
            {
                opened = true;
                break;
            }
        }

        if !opened {
            anyhow::bail!("No browser found. Please install Safari, Chrome, or Firefox.");
        }
    }

    #[cfg(target_os = "linux")]
    {
        let browsers = ["firefox", "google-chrome", "chromium", "brave"];
        let mut opened = false;

        for browser in &browsers {
            if std::process::Command::new(browser).arg(url).spawn().is_ok() {
                opened = true;
                break;
            }
        }

        if !opened {
            anyhow::bail!("No browser found. Please install Firefox or Chrome.");
        }
    }

    #[cfg(target_os = "windows")]
    {
        let browsers = [
            ("msedge.exe", vec![url]),
            ("chrome.exe", vec![url]),
            ("firefox.exe", vec![url]),
        ];

        let mut opened = false;
        for (browser, args) in &browsers {
            if std::process::Command::new(browser)
                .args(args)
                .spawn()
                .is_ok()
            {
                opened = true;
                break;
            }
        }

        if !opened {
            anyhow::bail!("No browser found. Please install Edge, Chrome, or Firefox.");
        }
    }

    Ok(())
}
