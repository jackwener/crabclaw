//! OpenAI OAuth 2.0 PKCE authentication flow.
//!
//! Implements the same flow used by OpenAI Codex CLI, allowing users to
//! authenticate with their ChatGPT Plus/Pro subscription instead of an API key.
//!
//! Flow:
//! 1. Generate PKCE code_verifier + code_challenge
//! 2. Start local HTTP server on localhost:1455
//! 3. Open browser to OpenAI auth endpoint
//! 4. User logs in, OpenAI redirects to localhost with auth code
//! 5. Exchange code for access_token + refresh_token
//! 6. Store tokens in ~/.crabclaw/auth.json

use std::path::PathBuf;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::core::error::{CrabClawError, Result};

// OpenAI OAuth endpoints (same as Codex CLI)
const AUTH_ENDPOINT: &str = "https://auth.openai.com/oauth/authorize";
const TOKEN_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const SCOPES: &str = "openid profile email offline_access";
const CALLBACK_PORT: u16 = 1455;

/// Stored OAuth tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenData {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    /// Unix timestamp when access_token expires.
    pub expires_at: Option<i64>,
}

impl TokenData {
    /// Check if the access token is expired (with 60s buffer).
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(exp) => chrono::Utc::now().timestamp() >= exp - 60,
            None => false, // assume valid if no expiry
        }
    }
}

/// Token response from OpenAI.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_in: Option<i64>,
    #[allow(dead_code)]
    token_type: Option<String>,
}

// ---------------------------------------------------------------------------
// Token Store (persisted to ~/.crabclaw/auth.json)
// ---------------------------------------------------------------------------

fn token_file_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".crabclaw")
        .join("auth.json")
}

/// Load saved tokens from disk.
pub fn load_tokens() -> Option<TokenData> {
    let path = token_file_path();
    if !path.exists() {
        return None;
    }
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

/// Save tokens to disk.
fn save_tokens(tokens: &TokenData) -> Result<()> {
    let path = token_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(CrabClawError::Io)?;
    }
    let json = serde_json::to_string_pretty(tokens)?;
    std::fs::write(&path, json).map_err(CrabClawError::Io)?;
    info!("tokens saved to {}", path.display());
    Ok(())
}

/// Remove stored tokens.
pub fn clear_tokens() -> Result<()> {
    let path = token_file_path();
    if path.exists() {
        std::fs::remove_file(&path).map_err(CrabClawError::Io)?;
        info!("tokens cleared");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// PKCE helpers
// ---------------------------------------------------------------------------

fn generate_code_verifier() -> String {
    let bytes: Vec<u8> = (0..32).map(|_| rand::random::<u8>()).collect();
    URL_SAFE_NO_PAD.encode(bytes)
}

fn generate_state() -> String {
    let bytes: Vec<u8> = (0..16).map(|_| rand::random::<u8>()).collect();
    URL_SAFE_NO_PAD.encode(bytes)
}

fn generate_code_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    URL_SAFE_NO_PAD.encode(hash)
}

// ---------------------------------------------------------------------------
// OAuth Login Flow
// ---------------------------------------------------------------------------

/// Run the full OAuth PKCE login flow:
/// 1. Open browser to OpenAI auth page
/// 2. Listen on localhost for callback with auth code
/// 3. Exchange code for tokens
/// 4. Save tokens to disk
pub async fn login() -> Result<TokenData> {
    let code_verifier = generate_code_verifier();
    let code_challenge = generate_code_challenge(&code_verifier);
    let state = generate_state();

    // Build authorization URL
    let auth_url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={}&code_challenge_method=S256&state={}",
        AUTH_ENDPOINT,
        CLIENT_ID,
        urlencoding(REDIRECT_URI),
        urlencoding(SCOPES),
        code_challenge,
        state,
    );

    // Start local server to receive callback
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{CALLBACK_PORT}"))
        .await
        .map_err(|e| {
            CrabClawError::Network(format!(
                "failed to bind localhost:{CALLBACK_PORT}: {e}. Is another instance running?"
            ))
        })?;

    println!("\n🔐 Opening browser for OpenAI login...");
    println!("   If the browser doesn't open, visit:\n   {auth_url}\n");

    // Open browser
    if let Err(e) = open::that(&auth_url) {
        warn!("failed to open browser: {e}");
    }

    // Wait for callback with auth code
    let auth_code = wait_for_callback(listener, &state).await?;
    info!("received auth code, exchanging for tokens");

    // Exchange code for tokens
    let tokens = exchange_code(&auth_code, &code_verifier).await?;
    save_tokens(&tokens)?;

    println!("✅ Login successful! Tokens saved to ~/.crabclaw/auth.json");
    Ok(tokens)
}

/// Wait for the OAuth callback on the local server.
async fn wait_for_callback(
    listener: tokio::net::TcpListener,
    expected_state: &str,
) -> Result<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let (mut stream, _) = listener
        .accept()
        .await
        .map_err(|e| CrabClawError::Network(format!("failed to accept connection: {e}")))?;

    let mut buf = vec![0u8; 4096];
    let n = stream
        .read(&mut buf)
        .await
        .map_err(|e| CrabClawError::Network(format!("failed to read request: {e}")))?;

    let request = String::from_utf8_lossy(&buf[..n]);

    // Check for error response first
    if let Some(error) = extract_query_param(&request, "error") {
        let desc = extract_query_param(&request, "error_description")
            .map(|d| d.replace('+', " "))
            .unwrap_or_default();

        let html =
            format!("<html><body><h2>❌ Login failed</h2><p>{error}: {desc}</p></body></html>");
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
            html.len(),
            html
        );
        let _ = stream.write_all(response.as_bytes()).await;

        return Err(CrabClawError::Auth(format!(
            "OAuth error: {error} — {desc}"
        )));
    }

    // Extract code
    let code = extract_query_param(&request, "code")
        .ok_or_else(|| CrabClawError::Auth("no authorization code in callback".to_string()))?;

    // Verify state for CSRF protection
    #[allow(clippy::collapsible_if)]
    if let Some(returned_state) = extract_query_param(&request, "state") {
        if returned_state != expected_state {
            return Err(CrabClawError::Auth(
                "state mismatch — possible CSRF".to_string(),
            ));
        }
    }

    // Send success response to browser
    let html = r#"<html><body><h2>✅ Login successful!</h2><p>You can close this tab and return to the terminal.</p><script>window.close()</script></body></html>"#;
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
        html.len(),
        html
    );
    let _ = stream.write_all(response.as_bytes()).await;

    Ok(code)
}

/// Extract a query parameter from an HTTP request line.
fn extract_query_param(request: &str, key: &str) -> Option<String> {
    let first_line = request.lines().next()?;
    let path = first_line.split_whitespace().nth(1)?;
    let query = path.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        if parts.next()? == key {
            return parts.next().map(|v| v.to_string());
        }
    }
    None
}

/// Exchange authorization code for tokens.
async fn exchange_code(code: &str, code_verifier: &str) -> Result<TokenData> {
    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_ENDPOINT)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", CLIENT_ID),
            ("code", code),
            ("redirect_uri", REDIRECT_URI),
            ("code_verifier", code_verifier),
        ])
        .send()
        .await
        .map_err(|e| CrabClawError::Network(format!("token exchange request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(CrabClawError::Auth(format!(
            "token exchange failed (HTTP {status}): {body}"
        )));
    }

    let token_resp: TokenResponse = resp
        .json()
        .await
        .map_err(|e| CrabClawError::Auth(format!("failed to parse token response: {e}")))?;

    let expires_at = token_resp
        .expires_in
        .map(|secs| chrono::Utc::now().timestamp() + secs);

    Ok(TokenData {
        access_token: token_resp.access_token,
        refresh_token: token_resp.refresh_token,
        id_token: token_resp.id_token,
        expires_at,
    })
}

/// Refresh an expired access token using the refresh token.
pub async fn refresh_access_token(tokens: &TokenData) -> Result<TokenData> {
    let refresh_token = tokens
        .refresh_token
        .as_ref()
        .ok_or_else(|| CrabClawError::Auth("no refresh token available".to_string()))?;

    info!("refreshing access token");

    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_ENDPOINT)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", CLIENT_ID),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await
        .map_err(|e| CrabClawError::Network(format!("token refresh failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(CrabClawError::Auth(format!(
            "token refresh failed (HTTP {status}): {body}. Please run `crabclaw auth login` again."
        )));
    }

    let token_resp: TokenResponse = resp
        .json()
        .await
        .map_err(|e| CrabClawError::Auth(format!("failed to parse refresh response: {e}")))?;

    let expires_at = token_resp
        .expires_in
        .map(|secs| chrono::Utc::now().timestamp() + secs);

    let new_tokens = TokenData {
        access_token: token_resp.access_token,
        refresh_token: token_resp.refresh_token.or(tokens.refresh_token.clone()),
        id_token: token_resp.id_token.or(tokens.id_token.clone()),
        expires_at,
    };

    save_tokens(&new_tokens)?;
    Ok(new_tokens)
}

/// Get a valid access token, refreshing if needed.
pub async fn get_valid_token() -> Result<String> {
    let tokens = load_tokens().ok_or_else(|| {
        CrabClawError::Auth("not logged in. Run `crabclaw auth login` first.".to_string())
    })?;

    if tokens.is_expired() {
        let refreshed = refresh_access_token(&tokens).await?;
        Ok(refreshed.access_token)
    } else {
        Ok(tokens.access_token)
    }
}

/// Print auth status.
pub fn status() {
    match load_tokens() {
        Some(tokens) => {
            println!("✅ Logged in via OAuth");
            if let Some(exp) = tokens.expires_at {
                let dt = chrono::DateTime::from_timestamp(exp, 0)
                    .map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                let expired = tokens.is_expired();
                println!(
                    "   Token expires:  {dt}{}",
                    if expired { " (EXPIRED)" } else { "" }
                );
                println!(
                    "   Refresh token:  {}",
                    if tokens.refresh_token.is_some() {
                        "present"
                    } else {
                        "none"
                    }
                );
            }
            println!("   Token file:     {}", token_file_path().display());
        }
        None => {
            println!("❌ Not logged in");
            println!("   Run `crabclaw auth login` to authenticate with your ChatGPT account.");
        }
    }
}

/// Minimal percent-encoding for URL params.
fn urlencoding(s: &str) -> String {
    s.replace(' ', "%20")
        .replace(':', "%3A")
        .replace('/', "%2F")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_code_challenge_is_deterministic() {
        let verifier = "test_verifier_12345";
        let c1 = generate_code_challenge(verifier);
        let c2 = generate_code_challenge(verifier);
        assert_eq!(c1, c2);
        assert!(!c1.is_empty());
    }

    #[test]
    fn pkce_verifier_is_random() {
        let v1 = generate_code_verifier();
        let v2 = generate_code_verifier();
        assert_ne!(v1, v2);
        assert!(v1.len() > 20);
    }

    #[test]
    fn extract_code_from_callback() {
        let request = "GET /auth/callback?code=abc123&state=xyz HTTP/1.1\r\nHost: localhost\r\n";
        assert_eq!(
            extract_query_param(request, "code"),
            Some("abc123".to_string())
        );
        assert_eq!(
            extract_query_param(request, "state"),
            Some("xyz".to_string())
        );
        assert_eq!(extract_query_param(request, "missing"), None);
    }

    #[test]
    fn token_expired_check() {
        let expired = TokenData {
            access_token: "test".to_string(),
            refresh_token: None,
            id_token: None,
            expires_at: Some(0), // epoch = definitely expired
        };
        assert!(expired.is_expired());

        let valid = TokenData {
            access_token: "test".to_string(),
            refresh_token: None,
            id_token: None,
            expires_at: Some(chrono::Utc::now().timestamp() + 3600),
        };
        assert!(!valid.is_expired());

        let no_expiry = TokenData {
            access_token: "test".to_string(),
            refresh_token: None,
            id_token: None,
            expires_at: None,
        };
        assert!(!no_expiry.is_expired());
    }

    #[test]
    fn urlencoding_works() {
        assert_eq!(urlencoding("hello world"), "hello%20world");
        assert_eq!(
            urlencoding("http://localhost:1455"),
            "http%3A%2F%2Flocalhost%3A1455"
        );
    }
}
