//! Gmail HTTP client: pagination, retry classification, and rate limiting.
//!
//! Gmail allows 250 quota units per user per second and `messages.get` costs 5,
//! so the effective ceiling is about 50 fetches per second. Concurrency is
//! capped at 10 in-flight gets, which stays under that with headroom for the
//! list calls sharing the same budget.

use std::time::Duration;

use serde::Deserialize;

const API_BASE: &str = "https://gmail.googleapis.com/gmail/v1/users/me";
pub const MAX_IN_FLIGHT_GETS: usize = 10;
const MAX_ATTEMPTS: usize = 5;

/// How a failed Gmail call should be treated. The distinction matters: a
/// rate-limit 403 and a permissions 403 share a status code and need opposite
/// handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// Retry with backoff: network, 429, 5xx, 403 rateLimitExceeded.
    Retryable,
    /// The access token needs refreshing, then one retry.
    Unauthorized,
    /// Re-consent required. 403 insufficientPermissions.
    NeedsReconsent,
    /// The checkpoint is too old for Gmail to replay. Resync instead.
    CheckpointTooOld,
    /// Nothing to retry.
    Permanent,
}

#[derive(Debug)]
pub struct ApiError {
    pub kind: FailureKind,
    pub message: String,
}

impl ApiError {
    fn new(kind: FailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

#[derive(Deserialize)]
struct GoogleErrorEnvelope {
    error: Option<GoogleError>,
}

#[derive(Deserialize)]
struct GoogleError {
    message: Option<String>,
    errors: Option<Vec<GoogleErrorDetail>>,
}

#[derive(Deserialize)]
struct GoogleErrorDetail {
    reason: Option<String>,
}

#[derive(Deserialize)]
pub struct Profile {
    #[serde(rename = "emailAddress")]
    pub email_address: String,
    #[serde(rename = "historyId")]
    pub history_id: Option<String>,
}

#[derive(Deserialize)]
pub struct LabelList {
    #[serde(default)]
    pub labels: Vec<Label>,
}

#[derive(Deserialize, Clone)]
pub struct Label {
    pub id: String,
    pub name: String,
    #[serde(rename = "type", default)]
    pub label_type: Option<String>,
}

#[derive(Deserialize)]
pub struct MessageList {
    #[serde(default)]
    pub messages: Vec<MessageRef>,
    #[serde(rename = "nextPageToken")]
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct MessageRef {
    pub id: String,
    // Present in the list response. The full thread id is read from the
    // messages.get payload, so nothing consumes this yet.
    #[serde(rename = "threadId")]
    #[allow(dead_code)]
    pub thread_id: Option<String>,
}

#[derive(Deserialize)]
pub struct RawMessage {
    pub id: String,
    #[serde(rename = "threadId")]
    pub thread_id: Option<String>,
    #[serde(rename = "labelIds", default)]
    pub label_ids: Vec<String>,
    #[serde(rename = "internalDate")]
    pub internal_date: Option<String>,
    pub raw: Option<String>,
}

pub struct GmailApi {
    client: reqwest::Client,
}

impl GmailApi {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }

    pub async fn profile(&self, access_token: &str) -> Result<Profile, ApiError> {
        self.get_json(access_token, format!("{API_BASE}/profile"))
            .await
    }

    pub async fn labels(&self, access_token: &str) -> Result<LabelList, ApiError> {
        self.get_json(access_token, format!("{API_BASE}/labels"))
            .await
    }

    pub async fn list_messages(
        &self,
        access_token: &str,
        label_id: &str,
        page_token: Option<&str>,
        page_size: usize,
    ) -> Result<MessageList, ApiError> {
        let mut url = format!(
            "{API_BASE}/messages?labelIds={label_id}&maxResults={}",
            page_size.min(500)
        );
        if let Some(token) = page_token {
            url.push_str("&pageToken=");
            url.push_str(token);
        }
        self.get_json(access_token, url).await
    }

    /// `format=RAW` rather than `FULL`. FULL still returns base64url part
    /// bodies that have to be reassembled, and a real parser over RAW handles
    /// the malformed-header tail better.
    pub async fn get_message_raw(
        &self,
        access_token: &str,
        message_id: &str,
    ) -> Result<RawMessage, ApiError> {
        self.get_json(
            access_token,
            format!("{API_BASE}/messages/{message_id}?format=RAW"),
        )
        .await
    }

    /// Exponential backoff with jitter, honouring `Retry-After` when present.
    async fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        access_token: &str,
        url: String,
    ) -> Result<T, ApiError> {
        let mut attempt = 0usize;

        loop {
            attempt += 1;
            let response = self.client.get(&url).bearer_auth(access_token).send().await;

            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    if attempt >= MAX_ATTEMPTS {
                        return Err(ApiError::new(
                            FailureKind::Retryable,
                            transport_message(&error),
                        ));
                    }
                    tokio::time::sleep(backoff_delay(attempt, None)).await;
                    continue;
                }
            };

            let status = response.status();
            if status.is_success() {
                return response.json::<T>().await.map_err(|_| {
                    ApiError::new(
                        FailureKind::Permanent,
                        "Gmail returned an unexpected response shape.",
                    )
                });
            }

            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .map(Duration::from_secs);

            let body = response.text().await.unwrap_or_default();
            let error = classify(status, &body);

            match error.kind {
                FailureKind::Retryable if attempt < MAX_ATTEMPTS => {
                    tokio::time::sleep(backoff_delay(attempt, retry_after)).await;
                    continue;
                }
                _ => return Err(error),
            }
        }
    }
}

/// 403 splits two ways on `reason`: a rate limit is retryable, insufficient
/// permissions needs a fresh consent and must not be retried.
pub fn classify(status: reqwest::StatusCode, body: &str) -> ApiError {
    let envelope: Option<GoogleErrorEnvelope> = serde_json::from_str(body).ok();
    let reason = envelope
        .as_ref()
        .and_then(|value| value.error.as_ref())
        .and_then(|error| error.errors.as_ref())
        .and_then(|errors| errors.first())
        .and_then(|detail| detail.reason.clone())
        .unwrap_or_default();
    let message = envelope
        .as_ref()
        .and_then(|value| value.error.as_ref())
        .and_then(|error| error.message.clone())
        .unwrap_or_else(|| status.to_string());

    match status.as_u16() {
        401 => ApiError::new(FailureKind::Unauthorized, "The Gmail session expired."),
        403 => match reason.as_str() {
            "rateLimitExceeded" | "userRateLimitExceeded" | "quotaExceeded" => {
                ApiError::new(FailureKind::Retryable, "Gmail is rate limiting Lotus.")
            }
            _ => ApiError::new(
                FailureKind::NeedsReconsent,
                "Lotus does not have permission for this mailbox. Reconnect the account.",
            ),
        },
        404 => ApiError::new(FailureKind::CheckpointTooOld, "Gmail could not find that."),
        429 => ApiError::new(FailureKind::Retryable, "Gmail is rate limiting Lotus."),
        500..=599 => ApiError::new(FailureKind::Retryable, "Gmail is temporarily unavailable."),
        _ => ApiError::new(FailureKind::Permanent, sanitize(&message)),
    }
}

/// Google error messages can echo request parameters. Keep the shape, drop
/// anything that looks like a URL or a token.
fn sanitize(message: &str) -> String {
    let cleaned: String = message
        .split_whitespace()
        .filter(|word| !word.contains("://") && word.len() < 60)
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.is_empty() {
        "Gmail rejected the request.".into()
    } else {
        cleaned
    }
}

fn transport_message(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "The Gmail request timed out.".into()
    } else if error.is_connect() {
        "No network connection to Gmail.".into()
    } else {
        "The Gmail request failed.".into()
    }
}

/// Jitter matters: without it, ten concurrent 429s retry in lockstep and hit the
/// same limit again.
pub fn backoff_delay(attempt: usize, retry_after: Option<Duration>) -> Duration {
    if let Some(after) = retry_after {
        return after.min(Duration::from_secs(60));
    }
    let base_millis = 250u64.saturating_mul(1 << attempt.min(6));
    let jitter = rand::random::<u64>() % (base_millis / 2 + 1);
    Duration::from_millis((base_millis + jitter).min(30_000))
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn rate_limit_403_is_retryable_but_permission_403_is_not() {
        let rate_limited = r#"{"error":{"message":"Rate Limit Exceeded",
            "errors":[{"reason":"rateLimitExceeded"}]}}"#;
        assert_eq!(
            classify(StatusCode::FORBIDDEN, rate_limited).kind,
            FailureKind::Retryable
        );

        let forbidden = r#"{"error":{"message":"Insufficient Permission",
            "errors":[{"reason":"insufficientPermissions"}]}}"#;
        assert_eq!(
            classify(StatusCode::FORBIDDEN, forbidden).kind,
            FailureKind::NeedsReconsent
        );
    }

    #[test]
    fn status_codes_map_to_the_expected_kinds() {
        assert_eq!(
            classify(StatusCode::UNAUTHORIZED, "{}").kind,
            FailureKind::Unauthorized
        );
        assert_eq!(
            classify(StatusCode::NOT_FOUND, "{}").kind,
            FailureKind::CheckpointTooOld
        );
        assert_eq!(
            classify(StatusCode::TOO_MANY_REQUESTS, "{}").kind,
            FailureKind::Retryable
        );
        assert_eq!(
            classify(StatusCode::INTERNAL_SERVER_ERROR, "{}").kind,
            FailureKind::Retryable
        );
        assert_eq!(
            classify(StatusCode::BAD_REQUEST, "{}").kind,
            FailureKind::Permanent
        );
    }

    #[test]
    fn error_messages_drop_urls_and_long_tokens() {
        let body =
            r#"{"error":{"message":"Invalid request https://gmail.googleapis.com/x?token=abc"}}"#;
        let error = classify(StatusCode::BAD_REQUEST, body);
        assert!(!error.message.contains("://"));
        assert!(error.message.contains("Invalid request"));
    }

    #[test]
    fn retry_after_wins_over_computed_backoff_and_is_capped() {
        assert_eq!(
            backoff_delay(1, Some(Duration::from_secs(7))),
            Duration::from_secs(7)
        );
        assert_eq!(
            backoff_delay(1, Some(Duration::from_secs(600))),
            Duration::from_secs(60)
        );
    }

    #[test]
    fn backoff_grows_and_stays_bounded() {
        let first = backoff_delay(1, None);
        let later = backoff_delay(5, None);
        assert!(later > first);
        assert!(later <= Duration::from_secs(30));
    }
}
