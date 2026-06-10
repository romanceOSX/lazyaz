//! Personal Access Token "authenticator". There's no interactive flow — the
//! token comes from the environment — so this just carries the [`AuthState`].

use super::{AuthState, Authenticator};
use anyhow::Result;

pub struct PatAuthenticator {
    state: AuthState,
}

impl PatAuthenticator {
    pub fn new(state: AuthState) -> Self {
        Self { state }
    }
}

impl Authenticator for PatAuthenticator {
    fn login(&mut self) -> Result<AuthState> {
        Ok(self.state.clone())
    }

    fn token(&self) -> Option<&str> {
        Some(&self.state.token)
    }

    fn account(&self) -> Option<&str> {
        Some(&self.state.account)
    }

    fn logout(&mut self) {}

    fn list_orgs(&self) -> Vec<String> {
        // PATs are org-scoped; org is taken from config rather than discovered.
        Vec::new()
    }
}
