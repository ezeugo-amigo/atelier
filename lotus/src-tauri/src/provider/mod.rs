//! The provider boundary. Every method is async so network calls happen outside
//! the storage lock: the lock is taken only to apply a delta.

pub mod gmail;
pub mod mock;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use crate::model::*;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait MailProvider: Send + Sync {
    fn option(&self) -> ProviderOption;

    fn begin_login(&self) -> BoxFuture<'_, Result<AccountLogin, String>>;

    /// Only meaningful for mock providers, where the user types an address.
    /// Browser providers complete through the OAuth callback instead.
    fn complete_login<'a>(
        &'a self,
        login_state: &'a str,
        email_address: &'a str,
    ) -> BoxFuture<'a, Result<AccountSeed, String>>;

    fn sync_mailbox<'a>(
        &'a self,
        state: &'a ProviderSyncState,
    ) -> BoxFuture<'a, Result<MailboxDelta, String>>;

    /// Downcast hook. The Gmail login flow needs the concrete provider, because
    /// the loopback callback and the browser launch do not fit the trait's
    /// request/response shape.
    fn as_any(&self) -> &dyn std::any::Any;
}

pub struct ProviderRegistry {
    providers: HashMap<ProviderKind, Box<dyn MailProvider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    pub fn with_mocks() -> Self {
        let mut registry = Self::new();
        registry.insert(Box::new(mock::MockMailProvider::gmail()));
        registry.insert(Box::new(mock::MockMailProvider::outlook()));
        registry
    }

    pub fn insert(&mut self, provider: Box<dyn MailProvider>) {
        let kind = provider.option().provider;
        self.providers.insert(kind, provider);
    }

    pub fn options(&self) -> Vec<ProviderOption> {
        let mut options: Vec<ProviderOption> = self
            .providers
            .values()
            .map(|provider| provider.option())
            .collect();
        // Real providers first, then alphabetical, so Gmail leads the grid.
        options.sort_by(|left, right| {
            right
                .browser_login
                .cmp(&left.browser_login)
                .then_with(|| left.display_name.cmp(&right.display_name))
        });
        options
    }

    pub fn get(&self, provider: ProviderKind) -> Result<&dyn MailProvider, String> {
        self.providers
            .get(&provider)
            .map(|provider| provider.as_ref())
            .ok_or_else(|| format!("Unsupported provider: {provider:?}"))
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}
