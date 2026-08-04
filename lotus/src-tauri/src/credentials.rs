//! Keychain-backed token storage.
//!
//! Refresh tokens live in the OS keychain and nowhere else. SQLite holds only
//! the keychain account key (`credential_ref`). Access tokens stay in memory,
//! keyed by account, and the refresh is single-flighted so a burst of parallel
//! Gmail calls produces one token request rather than a stampede.

use std::collections::HashMap;
use std::sync::Arc;

use keyring::Entry;
use tokio::sync::Mutex;

use crate::model::now_iso8601;

const SERVICE: &str = "ai.atelier.lotus.oauth.v1";

/// An access token plus the instant it stops being usable.
#[derive(Clone)]
pub struct AccessToken {
    pub value: String,
    pub expires_at_epoch: i64,
}

impl AccessToken {
    /// Treat a token as expired 60 seconds early, so a refresh happens before a
    /// request fails rather than after.
    fn is_fresh(&self, now_epoch: i64) -> bool {
        self.expires_at_epoch - 60 > now_epoch
    }
}

pub struct CredentialStore {
    access_tokens: Mutex<HashMap<String, AccessToken>>,
    /// One lock per account, so a refresh for account A does not block account B.
    refresh_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl CredentialStore {
    pub fn new() -> Self {
        Self {
            access_tokens: Mutex::new(HashMap::new()),
            refresh_locks: Mutex::new(HashMap::new()),
        }
    }

    fn entry(credential_ref: &str) -> Result<Entry, String> {
        Entry::new(SERVICE, credential_ref)
            .map_err(|error| format!("Could not reach the system keychain: {error}"))
    }

    pub fn save_refresh_token(
        &self,
        credential_ref: &str,
        refresh_token: &str,
    ) -> Result<(), String> {
        Self::entry(credential_ref)?
            .set_password(refresh_token)
            .map_err(|error| format!("Could not store the credential in the keychain: {error}"))
    }

    pub fn refresh_token(&self, credential_ref: &str) -> Result<String, String> {
        Self::entry(credential_ref)?
            .get_password()
            .map_err(|error| {
                format!("No stored credential for this account. Reconnect to continue. ({error})")
            })
    }

    pub fn delete_refresh_token(&self, credential_ref: &str) -> Result<(), String> {
        match Self::entry(credential_ref) {
            // A missing entry is the desired end state, so deleting twice is fine.
            Ok(entry) => match entry.delete_credential() {
                Ok(()) => Ok(()),
                Err(keyring::Error::NoEntry) => Ok(()),
                Err(error) => Err(format!("Could not remove the credential: {error}")),
            },
            Err(error) => Err(error),
        }
    }

    pub async fn cached_access_token(&self, account_id: &str, now_epoch: i64) -> Option<String> {
        self.access_tokens
            .lock()
            .await
            .get(account_id)
            .filter(|token| token.is_fresh(now_epoch))
            .map(|token| token.value.clone())
    }

    pub async fn store_access_token(&self, account_id: &str, token: AccessToken) {
        self.access_tokens
            .lock()
            .await
            .insert(account_id.to_string(), token);
    }

    pub async fn clear_access_token(&self, account_id: &str) {
        self.access_tokens.lock().await.remove(account_id);
    }

    /// The per-account gate for a refresh. Callers hold it, re-check the cache,
    /// and only then hit the token endpoint.
    pub async fn refresh_gate(&self, account_id: &str) -> Arc<Mutex<()>> {
        let mut locks = self.refresh_locks.lock().await;
        locks
            .entry(account_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

impl Default for CredentialStore {
    fn default() -> Self {
        Self::new()
    }
}

pub fn credential_ref(provider_slug: &str, provider_account_id: &str) -> String {
    format!("{provider_slug}:{provider_account_id}")
}

pub fn expiry_iso8601(expires_in_seconds: i64) -> String {
    let base = time::OffsetDateTime::now_utc() + time::Duration::seconds(expires_in_seconds);
    base.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| now_iso8601())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_token_expires_sixty_seconds_early() {
        let token = AccessToken {
            value: "value".into(),
            expires_at_epoch: 1_000,
        };
        assert!(token.is_fresh(900));
        // 60s guard: at 941 the token still has 59s of nominal life left.
        assert!(!token.is_fresh(941));
        assert!(!token.is_fresh(1_001));
    }

    #[test]
    fn credential_ref_is_provider_scoped() {
        assert_eq!(
            credential_ref("gmail", "reader@example.com"),
            "gmail:reader@example.com"
        );
    }
}
