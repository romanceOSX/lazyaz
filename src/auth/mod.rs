//! Authentication seam.
//!
//! The skeleton ships a [`mock::MockAuthenticator`]; the real Entra ID
//! device-code flow lives in [`oauth`] (stubbed for now). The UI only ever
//! talks to the [`Authenticator`] trait so the real impl can drop in later.

pub mod mock;
pub mod oauth;
pub mod pat;

use anyhow::Result;
use base64::Engine;

/// How the token authenticates HTTP requests to Azure DevOps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthScheme {
    /// Entra ID OAuth access token → `Authorization: Bearer <token>`.
    Bearer,
    /// Personal Access Token → `Authorization: Basic base64(":"<pat>)`.
    BasicPat,
}

/// The result of a successful sign-in.
#[derive(Clone, Debug)]
pub struct AuthState {
    pub account: String,
    pub token: String,
    pub scheme: AuthScheme,
}

impl AuthState {
    /// The value for the `Authorization` HTTP header.
    pub fn header(&self) -> String {
        match self.scheme {
            AuthScheme::Bearer => format!("Bearer {}", self.token),
            AuthScheme::BasicPat => {
                let raw = format!(":{}", self.token);
                let b64 = base64::engine::general_purpose::STANDARD.encode(raw);
                format!("Basic {b64}")
            }
        }
    }
}

#[allow(dead_code)] // token()/logout() exercised once real auth lands
pub trait Authenticator {
    /// Perform an interactive login (opens a browser in the real impl).
    fn login(&mut self) -> Result<AuthState>;
    /// Currently held bearer token, if signed in.
    fn token(&self) -> Option<&str>;
    fn account(&self) -> Option<&str>;
    fn logout(&mut self);
    /// Azure DevOps organizations the signed-in user can access (wizard fzf source).
    /// The real impl queries the accounts API; the mock returns a canned list.
    fn list_orgs(&self) -> Vec<String>;
}
