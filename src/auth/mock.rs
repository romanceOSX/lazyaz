use super::{AuthScheme, AuthState, Authenticator};
use anyhow::Result;

/// Returns a fake token instantly. No network, no browser.
#[derive(Default)]
pub struct MockAuthenticator {
    state: Option<AuthState>,
}

impl Authenticator for MockAuthenticator {
    fn login(&mut self) -> Result<AuthState> {
        let state = AuthState {
            account: "you@example.com".to_string(),
            token: "mock-token".to_string(),
            scheme: AuthScheme::Bearer,
        };
        self.state = Some(state.clone());
        Ok(state)
    }

    fn token(&self) -> Option<&str> {
        self.state.as_ref().map(|s| s.token.as_str())
    }

    fn account(&self) -> Option<&str> {
        self.state.as_ref().map(|s| s.account.as_str())
    }

    fn logout(&mut self) {
        self.state = None;
    }

    fn list_orgs(&self) -> Vec<String> {
        vec![
            "contoso".into(),
            "fabrikam".into(),
            "northwind-traders".into(),
            "acme-corp".into(),
        ]
    }
}
