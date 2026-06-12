//! Entra ID device-code authentication for Azure DevOps.
//!
//! Uses the public, pre-consented Azure CLI client so no app registration is
//! required. The flow: request a device code, show the user a URL + code, open
//! the browser, poll until they sign in, then cache the access token on disk.
//!
//! Token cache lives next to the config (`token.json`). This is plaintext on
//! disk — acceptable for a dev tool; OS-keychain storage is a future improvement.

use super::{AuthScheme, AuthState, Authenticator};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const CLIENT_ID: &str = "04b07795-8ddb-461a-bbee-02f9e1bf7b46";
/// `.default` of the Azure DevOps resource id.
pub const DEVOPS_SCOPE: &str = "499b84ac-1321-427f-aa17-267ca6975798/.default offline_access";
const TENANT: &str = "organizations";

pub struct OAuthAuthenticator {
    http: reqwest::blocking::Client,
    state: Option<AuthState>,
    /// Long-lived refresh token (from the `offline_access` scope) used to mint
    /// fresh access tokens silently — so the user only signs in interactively
    /// once every few months instead of ~hourly.
    refresh_token: Option<String>,
    account: String,
}

#[derive(Serialize, Deserialize)]
struct CachedToken {
    access_token: String,
    expires_at: u64,
    account: String,
    /// Persisted so a new access token can be minted without re-prompting.
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Deserialize)]
struct DeviceCodeResp {
    device_code: String,
    user_code: String,
    verification_uri: String,
    interval: u64,
    message: Option<String>,
}

#[derive(Deserialize)]
struct TokenResp {
    access_token: Option<String>,
    expires_in: Option<u64>,
    refresh_token: Option<String>,
    error: Option<String>,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache_path() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "lazyaz")
        .context("could not determine config directory")?;
    Ok(dirs.config_dir().join("token.json"))
}

impl OAuthAuthenticator {
    /// Build an authenticator, loading the cached token + refresh token. The
    /// access token is only adopted if still valid; the refresh token is kept
    /// regardless so [`ensure_token`](Self::ensure_token) can silently renew it.
    pub fn new() -> Self {
        let http = reqwest::blocking::Client::builder()
            .user_agent("lazyaz")
            .build()
            .unwrap_or_default();
        let cached = Self::load_cache();
        let (state, refresh_token, account) = match cached {
            Some(c) => {
                let valid = c.expires_at > now_unix() + 60;
                let state = valid.then(|| AuthState {
                    account: c.account.clone(),
                    token: c.access_token.clone(),
                    scheme: AuthScheme::Bearer,
                });
                (state, c.refresh_token, c.account)
            }
            None => (None, None, "Entra ID user".to_string()),
        };
        Self {
            http,
            state,
            refresh_token,
            account,
        }
    }

    fn load_cache() -> Option<CachedToken> {
        let path = cache_path().ok()?;
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn save_cache(state: &AuthState, expires_in: u64, refresh_token: Option<String>) {
        let cached = CachedToken {
            access_token: state.token.clone(),
            expires_at: now_unix() + expires_in,
            account: state.account.clone(),
            refresh_token,
        };
        if let Ok(path) = cache_path() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string(&cached) {
                let _ = std::fs::write(&path, json);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
                }
            }
        }
    }

    fn device_flow(&self) -> Result<(String, u64, Option<String>)> {
        let dc: DeviceCodeResp = self
            .http
            .post(format!(
                "https://login.microsoftonline.com/{TENANT}/oauth2/v2.0/devicecode"
            ))
            .form(&[("client_id", CLIENT_ID), ("scope", DEVOPS_SCOPE)])
            .send()
            .context("requesting device code")?
            .error_for_status()
            .context("device code request rejected")?
            .json()
            .context("parsing device code response")?;

        // Show the user what to do, copy the code to the clipboard, then open
        // the browser. The clipboard handle is kept alive for the duration of
        // the polling loop so the code survives on clipboard managers (notably
        // X11) that only serve content while the owning process is running.
        let msg = dc.message.clone().unwrap_or_else(|| {
            format!(
                "To sign in, open {} and enter code {}",
                dc.verification_uri, dc.user_code
            )
        });
        let _clipboard = match arboard::Clipboard::new() {
            Ok(mut cb) => match cb.set_text(dc.user_code.clone()) {
                Ok(()) => {
                    eprintln!(
                        "\n{msg}\n(verification code {} copied to clipboard)\n",
                        dc.user_code
                    );
                    Some(cb)
                }
                Err(_) => {
                    eprintln!("\n{msg}\n");
                    None
                }
            },
            Err(_) => {
                eprintln!("\n{msg}\n");
                None
            }
        };
        let _ = open::that(&dc.verification_uri);

        // Poll for the token.
        let mut interval = dc.interval.max(1);
        loop {
            std::thread::sleep(Duration::from_secs(interval));
            let resp: TokenResp = self
                .http
                .post(format!(
                    "https://login.microsoftonline.com/{TENANT}/oauth2/v2.0/token"
                ))
                .form(&[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("client_id", CLIENT_ID),
                    ("device_code", &dc.device_code),
                ])
                .send()
                .context("polling for token")?
                .json()
                .context("parsing token response")?;

            if let Some(token) = resp.access_token {
                return Ok((token, resp.expires_in.unwrap_or(3600), resp.refresh_token));
            }
            match resp.error.as_deref() {
                Some("authorization_pending") => {}
                Some("slow_down") => interval += 5,
                Some(other) => return Err(anyhow!("sign-in failed: {other}")),
                None => return Err(anyhow!("unexpected token response")),
            }
        }
    }

    /// Look up the user's organizations via the accounts API. Best-effort:
    /// returns an empty list if the token lacks profile scope.
    fn fetch_orgs(&self) -> Result<Vec<String>> {
        let token = self.token().ok_or_else(|| anyhow!("not signed in"))?;
        #[derive(Deserialize)]
        struct Profile {
            id: String,
        }
        #[derive(Deserialize)]
        struct Account {
            #[serde(rename = "accountName")]
            account_name: String,
        }
        #[derive(Deserialize)]
        struct Accounts {
            value: Vec<Account>,
        }
        let profile: Profile = self
            .http
            .get("https://app.vssps.visualstudio.com/_apis/profile/profiles/me?api-version=7.1")
            .bearer_auth(token)
            .send()?
            .error_for_status()?
            .json()?;
        let accounts: Accounts = self
            .http
            .get(format!(
                "https://app.vssps.visualstudio.com/_apis/accounts?memberId={}&api-version=7.1",
                profile.id
            ))
            .bearer_auth(token)
            .send()?
            .error_for_status()?
            .json()?;
        Ok(accounts.value.into_iter().map(|a| a.account_name).collect())
    }
}

impl Default for OAuthAuthenticator {
    fn default() -> Self {
        Self::new()
    }
}

impl OAuthAuthenticator {
    /// Ensure a usable access token, prompting the user as little as possible:
    /// a still-valid cached token is reused; an expired one is renewed silently
    /// via the refresh token; only if that fails (or `force_login`) do we run
    /// the interactive device-code flow.
    pub fn ensure_token(&mut self, force_login: bool) -> Result<()> {
        if force_login {
            self.login()?;
            return Ok(());
        }
        if self.state.is_some() {
            return Ok(()); // cached access token still valid
        }
        if self.refresh_token.is_some() && self.refresh().is_ok() {
            return Ok(()); // silently renewed — no browser needed
        }
        self.login()?;
        Ok(())
    }

    /// Exchange the refresh token for a fresh access token (and a rotated
    /// refresh token). Persists the result so the next launch renews silently.
    fn refresh(&mut self) -> Result<()> {
        let rt = self
            .refresh_token
            .clone()
            .ok_or_else(|| anyhow!("no refresh token"))?;
        let resp: TokenResp = self
            .http
            .post(format!(
                "https://login.microsoftonline.com/{TENANT}/oauth2/v2.0/token"
            ))
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", CLIENT_ID),
                ("refresh_token", rt.as_str()),
                ("scope", DEVOPS_SCOPE),
            ])
            .send()
            .context("refreshing access token")?
            .json()
            .context("parsing refresh response")?;
        let access = resp.access_token.ok_or_else(|| {
            anyhow!("token refresh rejected: {}", resp.error.unwrap_or_default())
        })?;
        let expires_in = resp.expires_in.unwrap_or(3600);
        // Entra rotates refresh tokens; fall back to the old one if absent.
        let new_rt = resp.refresh_token.or(Some(rt));
        let state = AuthState {
            account: self.account.clone(),
            token: access,
            scheme: AuthScheme::Bearer,
        };
        Self::save_cache(&state, expires_in, new_rt.clone());
        self.state = Some(state);
        self.refresh_token = new_rt;
        Ok(())
    }
}

impl Authenticator for OAuthAuthenticator {
    fn login(&mut self) -> Result<AuthState> {
        let (token, expires_in, refresh_token) = self.device_flow()?;
        let state = AuthState {
            account: self.account.clone(),
            token,
            scheme: AuthScheme::Bearer,
        };
        Self::save_cache(&state, expires_in, refresh_token.clone());
        self.state = Some(state.clone());
        self.refresh_token = refresh_token;
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
        if let Ok(path) = cache_path() {
            let _ = std::fs::remove_file(path);
        }
    }

    fn list_orgs(&self) -> Vec<String> {
        self.fetch_orgs().unwrap_or_default()
    }
}
