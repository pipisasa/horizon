//! GitHub and Google sign-in, for offline play only: the OAuth device-code flow, then who the
//! player is there (the provider's user id and name). Such an account opens the owner gate for
//! offline names, never for online servers or the download from Mojang, which still need a
//! Microsoft account that owns the game. It plays under an offline name the player chose at
//! sign-in.
//!
//! No token is kept: the keychain item `<provider>:<id>` only says the account signed in, so no
//! plain file can open the gate. The apps are configuration, at build time or run time:
//! `HORIZON_GITHUB_CLIENT_ID` (an OAuth app with device flow enabled), `HORIZON_GOOGLE_CLIENT_ID`
//! and `HORIZON_GOOGLE_CLIENT_SECRET` (a "TV and Limited Input devices" client; its secret is not
//! a secret for an installed app).
use super::{now, request_to, send, str_field, Body, DeviceCode, Poll, SecretStore};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    GitHub,
    Google,
}

impl Provider {
    pub const ALL: [Provider; 2] = [Provider::GitHub, Provider::Google];

    /// As accounts.json, the keychain and the CLI spell it.
    pub fn key(self) -> &'static str {
        match self {
            Provider::GitHub => "github",
            Provider::Google => "google",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Provider::GitHub => "GitHub",
            Provider::Google => "Google",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|p| p.key().eq_ignore_ascii_case(s))
    }
    pub fn client_id(self) -> Option<String> {
        match self {
            Provider::GitHub => config(
                "HORIZON_GITHUB_CLIENT_ID",
                option_env!("HORIZON_GITHUB_CLIENT_ID"),
            ),
            Provider::Google => config(
                "HORIZON_GOOGLE_CLIENT_ID",
                option_env!("HORIZON_GOOGLE_CLIENT_ID"),
            ),
        }
    }
    fn client_secret(self) -> Option<String> {
        match self {
            Provider::GitHub => None,
            Provider::Google => config(
                "HORIZON_GOOGLE_CLIENT_SECRET",
                option_env!("HORIZON_GOOGLE_CLIENT_SECRET"),
            ),
        }
    }
    /// Whether this build can sign in with it.
    pub fn configured(self) -> bool {
        self.client_id().is_some() && (self != Provider::Google || self.client_secret().is_some())
    }
    pub fn not_configured(self) -> String {
        match self {
            Provider::GitHub => {
                "This build of the launcher has no GitHub sign-in (HORIZON_GITHUB_CLIENT_ID)."
                    .into()
            }
            Provider::Google => "This build of the launcher has no Google sign-in \
                 (HORIZON_GOOGLE_CLIENT_ID and HORIZON_GOOGLE_CLIENT_SECRET)."
                .into(),
        }
    }
}

/// The run-time variable wins over the build-time one; empty means none.
fn config(var: &str, built: Option<&'static str>) -> Option<String> {
    std::env::var(var)
        .ok()
        .or_else(|| built.map(str::to_owned))
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

/// Where the requests go: the real services, or a mock in tests.
#[derive(Clone, Debug)]
pub struct Endpoints {
    /// github.com: `/login/device/code`, `/login/oauth/access_token`.
    pub github_web: String,
    /// api.github.com.
    pub github_api: String,
    /// oauth2.googleapis.com: `/device/code`, `/token`.
    pub google_oauth: String,
    /// The OpenID Connect userinfo endpoint.
    pub google_userinfo: String,
}

impl Endpoints {
    pub fn live() -> Self {
        Self {
            github_web: "https://github.com".into(),
            github_api: "https://api.github.com".into(),
            google_oauth: "https://oauth2.googleapis.com".into(),
            google_userinfo: "https://openidconnect.googleapis.com/v1/userinfo".into(),
        }
    }
}

/// Who signed in, before they chose the name they play under.
#[derive(Clone, Debug, PartialEq)]
pub struct Identity {
    pub provider: Provider,
    /// The provider's own user id (GitHub's number, Google's `sub`): stable across renames.
    pub id: String,
    /// The name the provider shows (GitHub login, Google name).
    pub login: String,
}

impl Identity {
    /// A name to offer: the login with what offline names can't hold left out, cut to 16.
    pub fn suggested_name(&self) -> String {
        let name: String = self
            .login
            .chars()
            .map(|c| if c == '-' || c == ' ' { '_' } else { c })
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .take(16)
            .collect();
        if name.chars().count() >= 3 {
            name
        } else {
            format!("{name}_player").chars().take(16).collect()
        }
    }
}

/// A GitHub or Google account in accounts.json: no secret, the keychain item says it signed in.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct External {
    pub provider: Provider,
    pub id: String,
    pub login: String,
    /// The offline name it plays under (one of `Accounts::offline`).
    pub name: String,
}

impl External {
    /// Its keychain item.
    pub fn key(&self) -> String {
        format!("{}:{}", self.provider.key(), self.id)
    }
}

/// The keychain's word that `account` signed in.
pub fn signed_in(store: &dyn SecretStore, account: &External) -> bool {
    store.get(&account.key()).ok().flatten().is_some()
}

/// Keeps the sign-in in the keychain.
pub(super) fn remember(store: &dyn SecretStore, account: &External) -> Result<(), String> {
    store.set(
        &account.key(),
        &json!({ "signed_in_at": now() }).to_string(),
    )
}

fn oauth_error(provider: Provider, v: &Value) -> String {
    match v.get("error").and_then(Value::as_str) {
        Some("access_denied") => "Sign-in was declined in the browser.".into(),
        Some("expired_token") => "The sign-in code expired. Start again.".into(),
        Some("incorrect_device_code" | "invalid_grant") => {
            "The sign-in code was not recognised. Start again.".into()
        }
        Some("device_flow_disabled") => {
            "The GitHub app of this launcher doesn't allow device sign-in.".into()
        }
        Some("invalid_client" | "unauthorized_client" | "incorrect_client_credentials") => format!(
            "{} refused this launcher's app registration (client ID).",
            provider.label()
        ),
        Some(other) => format!("{} sign-in failed ({other}).", provider.label()),
        None => format!("{} sign-in failed.", provider.label()),
    }
}

pub fn device_code(ep: &Endpoints, provider: Provider) -> Result<DeviceCode, String> {
    let client_id = provider
        .client_id()
        .ok_or_else(|| provider.not_configured())?;
    let (url, scope) = match provider {
        Provider::GitHub => (format!("{}/login/device/code", ep.github_web), ""),
        Provider::Google => (format!("{}/device/code", ep.google_oauth), "openid profile"),
    };
    let (status, v) = send(
        request_to("POST", &url).set("Accept", "application/json"),
        Some(Body::Form(vec![
            ("client_id", client_id.as_str()),
            ("scope", scope),
        ])),
        provider.label(),
    )?;
    match (
        status,
        str_field(&v, "user_code"),
        str_field(&v, "device_code"),
    ) {
        (200, Some(user_code), Some(device_code)) => Ok(DeviceCode {
            user_code,
            // Google says `verification_url`.
            verification_uri: str_field(&v, "verification_uri")
                .or_else(|| str_field(&v, "verification_url"))
                .unwrap_or_else(|| match provider {
                    Provider::GitHub => "https://github.com/login/device".into(),
                    Provider::Google => "https://www.google.com/device".into(),
                }),
            interval: v
                .get("interval")
                .and_then(Value::as_u64)
                .unwrap_or(5)
                .max(1),
            expires_at: now() + v.get("expires_in").and_then(Value::as_u64).unwrap_or(900),
            device_code,
        }),
        _ => Err(oauth_error(provider, &v)),
    }
}

/// One poll of the token endpoint: the access token once the player approved.
pub fn poll(ep: &Endpoints, provider: Provider, code: &DeviceCode) -> Result<Poll<String>, String> {
    let client_id = provider
        .client_id()
        .ok_or_else(|| provider.not_configured())?;
    let secret = provider.client_secret();
    let url = match provider {
        Provider::GitHub => format!("{}/login/oauth/access_token", ep.github_web),
        Provider::Google => format!("{}/token", ep.google_oauth),
    };
    let mut form = vec![
        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ("client_id", client_id.as_str()),
        ("device_code", code.device_code.as_str()),
    ];
    if let Some(secret) = &secret {
        form.push(("client_secret", secret.as_str()));
    }
    let (status, v) = send(
        request_to("POST", &url).set("Accept", "application/json"),
        Some(Body::Form(form)),
        provider.label(),
    )?;
    match v.get("error").and_then(Value::as_str) {
        Some("authorization_pending") => Ok(Poll::Pending),
        Some("slow_down") => Ok(Poll::SlowDown),
        _ => match (status, str_field(&v, "access_token")) {
            (200, Some(token)) => Ok(Poll::Done(token)),
            _ => Err(oauth_error(provider, &v)),
        },
    }
}

/// Who the token belongs to.
pub fn identity(ep: &Endpoints, provider: Provider, token: &str) -> Result<Identity, String> {
    let url = match provider {
        Provider::GitHub => format!("{}/user", ep.github_api),
        Provider::Google => ep.google_userinfo.clone(),
    };
    let (status, v) = send(
        request_to("GET", &url)
            .set("Accept", "application/json")
            .set("Authorization", &format!("Bearer {token}")),
        None,
        provider.label(),
    )?;
    let (id, login) = match provider {
        Provider::GitHub => (
            v.get("id").and_then(Value::as_u64).map(|n| n.to_string()),
            str_field(&v, "login"),
        ),
        Provider::Google => (
            str_field(&v, "sub"),
            str_field(&v, "name").or_else(|| str_field(&v, "given_name")),
        ),
    };
    match (status, id) {
        (200, Some(id)) => Ok(Identity {
            provider,
            login: login.unwrap_or_else(|| id.clone()),
            id,
        }),
        _ => Err(format!("Could not read the {} account.", provider.label())),
    }
}

/// From the device code to who signed in, polling as asked until the code expires or
/// `cancelled` says stop.
pub fn wait_for_identity(
    ep: &Endpoints,
    provider: Provider,
    code: &DeviceCode,
    cancelled: &dyn Fn() -> bool,
) -> Result<Identity, String> {
    let token = super::wait_for(code, cancelled, &|| poll(ep, provider, code))?;
    identity(ep, provider, &token)
}
