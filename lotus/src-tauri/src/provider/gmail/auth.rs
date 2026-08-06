//! Loopback OAuth with PKCE for a Desktop-app client.
//!
//! Google accepts any port on the loopback address for a Desktop client, so the
//! listener binds `127.0.0.1:0` and reads the assigned port. Two details are
//! easy to get wrong and both fail late:
//!
//! The redirect URI uses the IP literal `127.0.0.1`, never `localhost`. Google's
//! Desktop-client guidance steers to the literal, and the hostname fails consent
//! with `redirect_uri_mismatch`.
//!
//! The listener binds *before* the browser opens. macOS shows a firewall prompt
//! the first time an app binds a listening socket. Bind afterwards and the prompt
//! lands behind the consent screen mid-flow, where it reads as a crash.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

/// Requested up front, not escalated later. Moving `gmail.readonly` to
/// `gmail.modify` invalidates the existing grant and forces re-consent, so the
/// write scope costs one extra consent line now and saves a migration later.
pub const GMAIL_SCOPE: &str = "https://www.googleapis.com/auth/gmail.modify";
const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const SESSION_TIMEOUT: Duration = Duration::from_secs(300);

/// A Desktop-app OAuth client, loaded from `LOTUS_GOOGLE_CLIENT_CONFIG`.
#[derive(Clone)]
pub struct ClientConfig {
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Deserialize)]
struct ClientConfigFile {
    installed: Option<ClientConfigBody>,
    web: Option<ClientConfigBody>,
}

#[derive(Deserialize)]
struct ClientConfigBody {
    client_id: String,
    client_secret: String,
}

impl ClientConfig {
    /// A Desktop-app client secret is not secret in a distributed build. PKCE is
    /// what protects the flow; the env var keeps the value out of source.
    pub fn from_env() -> Result<Self, String> {
        let path = std::env::var("LOTUS_GOOGLE_CLIENT_CONFIG").map_err(|_| {
            "Gmail is not configured. Set LOTUS_GOOGLE_CLIENT_CONFIG to the path of your \
             Google Desktop-app OAuth client JSON, then restart Lotus."
                .to_string()
        })?;

        let raw = std::fs::read_to_string(&path).map_err(|error| {
            format!("Could not read the Google client configuration at {path}: {error}")
        })?;

        let parsed: ClientConfigFile = serde_json::from_str(&raw).map_err(|error| {
            format!("The Google client configuration at {path} is not valid JSON: {error}")
        })?;

        let body = parsed.installed.or(parsed.web).ok_or_else(|| {
            format!(
                "The Google client configuration at {path} has no \"installed\" section. \
                 Create a Desktop app OAuth client and download that file."
            )
        })?;

        if body.client_id.is_empty() {
            return Err(format!("The client_id in {path} is empty."));
        }

        Ok(Self {
            client_id: body.client_id,
            client_secret: body.client_secret,
        })
    }
}

/// One login attempt. The `state` value doubles as the correlation id Elm uses
/// to match a callback event to the flow it is showing.
pub struct PendingLogin {
    pub state: String,
    pub verifier: String,
    pub redirect_uri: String,
    pub authorization_url: String,
    listener: TcpListener,
}

pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: i64,
}

#[derive(Deserialize)]
struct TokenBody {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

#[derive(Deserialize)]
struct TokenError {
    error: Option<String>,
    error_description: Option<String>,
}

pub fn random_urlsafe(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buffer);
    URL_SAFE_NO_PAD.encode(buffer)
}

pub fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

/// Constant-time comparison, so a mismatched `state` cannot be probed byte by
/// byte through response timing.
pub fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        difference |= a ^ b;
    }
    difference == 0
}

impl PendingLogin {
    pub async fn begin(config: &ClientConfig) -> Result<Self, String> {
        // Bind first. The macOS firewall prompt must land while the user is
        // still looking at Lotus, not behind Google's consent screen.
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .map_err(|error| {
                format!("Could not open a local callback port for Google sign-in: {error}")
            })?;

        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        // The IP literal, never "localhost".
        let redirect_uri = format!("http://127.0.0.1:{port}");

        let state = random_urlsafe(32);
        let verifier = random_urlsafe(64);
        let challenge = pkce_challenge(&verifier);

        let mut url = url::Url::parse(AUTH_ENDPOINT).map_err(|error| error.to_string())?;
        url.query_pairs_mut()
            .append_pair("client_id", &config.client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", GMAIL_SCOPE)
            // access_type=offline plus prompt=consent is what reliably returns a
            // refresh token. Without prompt=consent, a repeat authorization
            // returns an access token only.
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent")
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");

        Ok(Self {
            state,
            verifier,
            redirect_uri,
            authorization_url: url.to_string(),
            listener,
        })
    }

    /// Await the browser redirect. Returns the authorization code once `state`
    /// matches. Times out after five minutes and drops the listener.
    pub async fn wait_for_code(self) -> Result<(String, String, String), String> {
        let expected_state = self.state.clone();
        let verifier = self.verifier.clone();
        let redirect_uri = self.redirect_uri.clone();
        let listener = self.listener;

        let accepted = tokio::time::timeout(SESSION_TIMEOUT, async {
            loop {
                let (mut stream, _) = listener
                    .accept()
                    .await
                    .map_err(|error| format!("Callback connection failed: {error}"))?;

                let request_line = {
                    let mut reader = BufReader::new(&mut stream);
                    let mut line = String::new();
                    reader
                        .read_line(&mut line)
                        .await
                        .map_err(|error| format!("Could not read the callback request: {error}"))?;
                    line
                };

                let query = parse_request_target(&request_line);
                // Browsers request /favicon.ico against the same port. Ignore
                // anything that is not the redirect and keep listening.
                if query.is_empty() {
                    let _ = respond(&mut stream, "Waiting for Google...").await;
                    continue;
                }

                let parameters = parse_query(&query);

                if let Some(error) = parameters.get("error") {
                    let _ = respond(&mut stream, "Sign-in was cancelled.").await;
                    return Err(match error.as_str() {
                        "access_denied" => "Sign-in was cancelled in the browser.".to_string(),
                        other => format!("Google returned an error during sign-in: {other}"),
                    });
                }

                let state = parameters.get("state").cloned().unwrap_or_default();
                if !constant_time_eq(&state, &expected_state) {
                    let _ = respond(&mut stream, "This sign-in link is not valid.").await;
                    return Err(
                        "The sign-in response did not match this request. No account was \
                         connected. Try again."
                            .to_string(),
                    );
                }

                let code = match parameters.get("code") {
                    Some(code) if !code.is_empty() => code.clone(),
                    _ => {
                        let _ = respond(&mut stream, "This sign-in link is not valid.").await;
                        return Err("Google did not return an authorization code.".to_string());
                    }
                };

                let _ = respond(
                    &mut stream,
                    "Lotus is connected. You can close this tab and return to the app.",
                )
                .await;
                return Ok(code);
            }
        })
        .await;

        match accepted {
            Ok(Ok(code)) => Ok((code, verifier, redirect_uri)),
            Ok(Err(error)) => Err(error),
            Err(_) => Err("Google sign-in timed out after five minutes. Try again.".to_string()),
        }
    }
}

pub async fn exchange_code(
    client: &reqwest::Client,
    config: &ClientConfig,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse, String> {
    let form = [
        ("client_id", config.client_id.as_str()),
        ("client_secret", config.client_secret.as_str()),
        ("code", code),
        ("code_verifier", verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri),
    ];
    post_token(client, &form).await
}

pub async fn refresh_access_token(
    client: &reqwest::Client,
    config: &ClientConfig,
    refresh_token: &str,
) -> Result<TokenResponse, String> {
    let form = [
        ("client_id", config.client_id.as_str()),
        ("client_secret", config.client_secret.as_str()),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ];
    post_token(client, &form).await
}

async fn post_token(
    client: &reqwest::Client,
    form: &[(&str, &str)],
) -> Result<TokenResponse, String> {
    let response = client
        .post(TOKEN_ENDPOINT)
        .form(form)
        .send()
        .await
        .map_err(|error| {
            format!(
                "Could not reach Google to complete sign-in: {}",
                safe(error)
            )
        })?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        let parsed: Option<TokenError> = serde_json::from_str(&body).ok();
        let kind = parsed
            .as_ref()
            .and_then(|error| error.error.clone())
            .unwrap_or_else(|| status.to_string());
        // invalid_grant is the revoked or expired case, and the only recovery is
        // a fresh consent. Name it so the caller can mark the account
        // disconnected instead of retrying.
        if kind == "invalid_grant" {
            return Err("invalid_grant".to_string());
        }
        let detail = parsed
            .and_then(|error| error.error_description)
            .unwrap_or(kind);
        return Err(format!("Google rejected the sign-in: {detail}"));
    }

    let parsed: TokenBody = serde_json::from_str(&body)
        .map_err(|_| "Google returned an unexpected sign-in response.".to_string())?;

    Ok(TokenResponse {
        access_token: parsed.access_token,
        refresh_token: parsed.refresh_token,
        expires_in: parsed.expires_in.unwrap_or(3600),
    })
}

/// Never surface a raw transport error: the URL can carry request data.
fn safe(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "the request timed out".into()
    } else if error.is_connect() {
        "no network connection".into()
    } else {
        "the request failed".into()
    }
}

async fn respond(stream: &mut tokio::net::TcpStream, message: &str) -> std::io::Result<()> {
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Lotus</title>\
         <style>body{{font-family:-apple-system,system-ui,sans-serif;margin:0;display:flex;\
         height:100vh;align-items:center;justify-content:center;background:#faf9f7;color:#2c2a28}}\
         p{{font-size:15px}}</style></head><body><p>{message}</p></body></html>"
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await
}

/// Pull the query string out of `GET /?code=...&state=... HTTP/1.1`.
fn parse_request_target(request_line: &str) -> String {
    request_line
        .split_whitespace()
        .nth(1)
        .and_then(|target| target.split_once('?'))
        .map(|(_, query)| query.to_string())
        .unwrap_or_default()
}

fn parse_query(query: &str) -> HashMap<String, String> {
    url::form_urlencoded::parse(query.as_bytes())
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_matches_rfc7636_example() {
        // The verifier and challenge from RFC 7636 appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            pkce_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn random_values_differ_and_are_urlsafe() {
        let first = random_urlsafe(32);
        let second = random_urlsafe(32);
        assert_ne!(first, second);
        assert!(first
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn state_comparison_rejects_mismatch_and_length_difference() {
        assert!(constant_time_eq("abc123", "abc123"));
        assert!(!constant_time_eq("abc123", "abc124"));
        assert!(!constant_time_eq("abc123", "abc12"));
        assert!(!constant_time_eq("", "a"));
    }

    #[test]
    fn request_target_yields_the_query_string() {
        assert_eq!(
            parse_request_target("GET /?code=abc&state=xyz HTTP/1.1\r\n"),
            "code=abc&state=xyz"
        );
        // A favicon request carries no query and must not be mistaken for the
        // redirect.
        assert_eq!(parse_request_target("GET /favicon.ico HTTP/1.1\r\n"), "");
    }

    #[test]
    fn query_parsing_decodes_percent_escapes() {
        let parsed = parse_query("code=4%2F0Ab&state=a-b_c&scope=https%3A%2F%2Fmail");
        assert_eq!(parsed.get("code").unwrap(), "4/0Ab");
        assert_eq!(parsed.get("state").unwrap(), "a-b_c");
        assert_eq!(parsed.get("scope").unwrap(), "https://mail");
    }

    #[tokio::test]
    async fn begin_uses_the_ip_literal_and_requests_offline_consent() {
        let config = ClientConfig {
            client_id: "test-client".into(),
            client_secret: "test-secret".into(),
        };
        let pending = PendingLogin::begin(&config).await.unwrap();

        assert!(pending.redirect_uri.starts_with("http://127.0.0.1:"));
        assert!(!pending.redirect_uri.contains("localhost"));

        let url = pending.authorization_url;
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("prompt=consent"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("gmail.modify"));
        assert_eq!(pending.state.len(), 43);
    }
}
