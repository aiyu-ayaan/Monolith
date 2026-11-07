//! OAuth 2.0 / OIDC sign-in for app users (doc 08 §3): authorization-code flow with PKCE.
//!
//! `oauth_start` stores a pending sign-in (state, PKCE verifier, where to send the browser back)
//! in memory for 10 minutes and returns the provider's authorize URL. `oauth_callback` swaps
//! the code for a token, reads the profile, finds or creates the user, and returns the URL to
//! send the browser to: the app's `redirectUri` with the session in the fragment
//! (`#token=…&refreshToken=…&expiresAt=…`) or `#error=…`. The redirect URI must match the
//! app's allowed redirects (open-redirect guard).
//!
//! Linking: a known (provider, subject) signs in that user. Otherwise a provider-verified email
//! links to the existing account with that email; an unverified email that is already taken is
//! refused (409) rather than linked. Supported: google, github, microsoft, facebook, discord,
//! gitlab, linkedin, twitch, x, generic oidc. Not yet: Apple (needs a signed client secret),
//! id-token signature checks (the profile comes from the userinfo endpoint over TLS), refresh of
//! provider tokens, unlinking, linking to a signed-in user.

use std::time::Duration;

use monolith_auth::{base64url, random_bytes, sha256};
use monolith_store::{AppStore, Id, Query, Value};
use serde_json::Value as J;

use crate::app::now_us;
use crate::auth_config::StoredProvider;
use crate::error::{CoreError, CoreResult};
use crate::system::{Registry, sys_write};

const PENDING_TTL_US: i64 = 600 * 1_000_000;
const MAX_PENDING: usize = 2_000;

pub(crate) struct Pending {
    app: String,
    provider: String,
    redirect_uri: String,
    callback: String,
    verifier: String,
    exp: i64,
}

/// Providers whose code exchange is implemented.
pub fn supported(id: &str) -> bool {
    id != "apple"
}

struct Endpoints {
    auth: String,
    token: String,
    userinfo: String,
    scope: &'static str,
    /// Client credentials go in a Basic header instead of the body.
    basic: bool,
}

struct Profile {
    subject: String,
    email: Option<String>,
    verified: bool,
    name: String,
}

fn ep(auth: &str, token: &str, userinfo: &str, scope: &'static str) -> Endpoints {
    Endpoints {
        auth: auth.into(),
        token: token.into(),
        userinfo: userinfo.into(),
        scope,
        basic: false,
    }
}

fn s(v: &J, k: &str) -> Option<String> {
    match &v[k] {
        J::String(t) if !t.is_empty() => Some(t.clone()),
        J::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn bad(msg: impl Into<String>) -> CoreError {
    CoreError::BadRequest(msg.into())
}

impl Registry {
    async fn endpoints(&self, p: &StoredProvider) -> CoreResult<Endpoints> {
        let extra = |k: &str| p.extra.get(k).cloned();
        Ok(match p.id.as_str() {
            "google" => ep(
                "https://accounts.google.com/o/oauth2/v2/auth",
                "https://oauth2.googleapis.com/token",
                "https://openidconnect.googleapis.com/v1/userinfo",
                "openid email profile",
            ),
            "github" => ep(
                "https://github.com/login/oauth/authorize",
                "https://github.com/login/oauth/access_token",
                "https://api.github.com/user",
                "read:user user:email",
            ),
            "microsoft" => {
                let t = extra("tenant").unwrap_or_else(|| "common".into());
                ep(
                    &format!("https://login.microsoftonline.com/{t}/oauth2/v2.0/authorize"),
                    &format!("https://login.microsoftonline.com/{t}/oauth2/v2.0/token"),
                    "https://graph.microsoft.com/oidc/userinfo",
                    "openid email profile",
                )
            }
            "facebook" => ep(
                "https://www.facebook.com/v19.0/dialog/oauth",
                "https://graph.facebook.com/v19.0/oauth/access_token",
                "https://graph.facebook.com/me?fields=id,name,email",
                "email public_profile",
            ),
            "discord" => ep(
                "https://discord.com/oauth2/authorize",
                "https://discord.com/api/oauth2/token",
                "https://discord.com/api/users/@me",
                "identify email",
            ),
            "gitlab" => {
                let b = extra("baseUrl").unwrap_or_else(|| "https://gitlab.com".into());
                let b = b.trim_end_matches('/');
                ep(
                    &format!("{b}/oauth/authorize"),
                    &format!("{b}/oauth/token"),
                    &format!("{b}/oauth/userinfo"),
                    "openid email profile",
                )
            }
            "linkedin" => ep(
                "https://www.linkedin.com/oauth/v2/authorization",
                "https://www.linkedin.com/oauth/v2/accessToken",
                "https://api.linkedin.com/v2/userinfo",
                "openid profile email",
            ),
            "twitch" => ep(
                "https://id.twitch.tv/oauth2/authorize",
                "https://id.twitch.tv/oauth2/token",
                "https://api.twitch.tv/helix/users",
                "user:read:email",
            ),
            "x" => Endpoints {
                basic: true,
                ..ep(
                    "https://twitter.com/i/oauth2/authorize",
                    "https://api.twitter.com/2/oauth2/token",
                    "https://api.twitter.com/2/users/me",
                    "users.read tweet.read",
                )
            },
            "oidc" => {
                let issuer = extra("issuer").ok_or_else(|| bad("the OIDC issuer is not set"))?;
                let url = format!(
                    "{}/.well-known/openid-configuration",
                    issuer.trim_end_matches('/')
                );
                let d: J = self
                    .wh
                    .client()
                    .get(&url)
                    .timeout(Duration::from_secs(10))
                    .send()
                    .await
                    .and_then(|r| r.error_for_status())
                    .map_err(|e| bad(format!("OIDC discovery failed: {}", e.without_url())))?
                    .json()
                    .await
                    .map_err(|e| bad(format!("OIDC discovery returned invalid JSON: {e}")))?;
                let get =
                    |k: &str| s(&d, k).ok_or_else(|| bad(format!("OIDC discovery lacks {k}")));
                ep(
                    &get("authorization_endpoint")?,
                    &get("token_endpoint")?,
                    &get("userinfo_endpoint")?,
                    "openid email profile",
                )
            }
            _ => return Err(bad("this provider is not supported yet")),
        })
    }

    /// Returns the provider URL to send the browser to. `origin` is this server's public origin.
    pub async fn oauth_start(
        &self,
        app: &str,
        provider: &str,
        redirect_uri: &str,
        origin: &str,
    ) -> CoreResult<String> {
        self.app_info(app).await?;
        let cfg = self.stored_auth(app).await?;
        let p = cfg
            .enabled(provider)
            .filter(|_| supported(provider))
            .ok_or(CoreError::NotFound("sign-in provider"))?;
        if cfg.allowed_redirects.is_empty() {
            return Err(bad(
                "no allowed redirect URLs are configured; add your app's URL under Authentication → Allowed redirects",
            ));
        }
        if !cfg.redirect_allowed(redirect_uri) {
            return Err(bad("redirectUri is not in the app's allowed redirect list"));
        }
        let e = self.endpoints(p).await?;
        let state =
            base64url(&random_bytes::<24>().map_err(|e| CoreError::Internal(e.to_string()))?);
        let verifier =
            base64url(&random_bytes::<32>().map_err(|e| CoreError::Internal(e.to_string()))?);
        let callback = format!(
            "{}/api/v1/apps/{app}/auth/oauth/{provider}/callback",
            origin.trim_end_matches('/')
        );
        let now = now_us();
        {
            let mut g = self.oauth_pending.lock().unwrap_or_else(|e| e.into_inner());
            g.retain(|_, v| v.exp > now);
            if g.len() >= MAX_PENDING {
                return Err(CoreError::Busy);
            }
            g.insert(
                state.clone(),
                Pending {
                    app: app.to_owned(),
                    provider: provider.to_owned(),
                    redirect_uri: redirect_uri.to_owned(),
                    callback: callback.clone(),
                    verifier: verifier.clone(),
                    exp: now + PENDING_TTL_US,
                },
            );
        }
        let mut url =
            reqwest::Url::parse(&e.auth).map_err(|e| CoreError::Internal(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &p.client_id)
            .append_pair("redirect_uri", &callback)
            .append_pair("scope", e.scope)
            .append_pair("state", &state)
            .append_pair("code_challenge", &base64url(&sha256(verifier.as_bytes())))
            .append_pair("code_challenge_method", "S256");
        Ok(url.into())
    }

    /// Finishes a sign-in and returns the URL for the browser: the app's redirect with the
    /// session (or an error) in the fragment. A bad `state` has nowhere safe to go: it is an error.
    pub async fn oauth_callback(
        &self,
        app: &str,
        provider: &str,
        code: Option<&str>,
        state: Option<&str>,
        error: Option<&str>,
    ) -> CoreResult<String> {
        let pending = state
            .and_then(|st| {
                self.oauth_pending
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(st)
            })
            .filter(|p| p.exp > now_us() && p.app == app && p.provider == provider)
            .ok_or_else(|| bad("the sign-in expired or was not started here; try again"))?;
        let fragment = match (error, code) {
            (Some(e), _) => Err(format!("the provider reported: {e}")),
            (None, None) => Err("the provider sent no code".to_owned()),
            (None, Some(code)) => self
                .oauth_finish(app, provider, code, &pending)
                .await
                .map_err(|e| e.to_string()),
        };
        let frag = match fragment {
            Ok(sess) => format!(
                "token={}&refreshToken={}&expiresAt={}",
                enc(&sess.token),
                enc(&sess.refresh_token),
                enc(&sess.expires_at)
            ),
            Err(msg) => format!("error=oauth_failed&error_description={}", enc(&msg)),
        };
        let base = pending.redirect_uri.split('#').next().unwrap_or_default();
        Ok(format!("{base}#{frag}"))
    }

    async fn oauth_finish(
        &self,
        app: &str,
        provider: &str,
        code: &str,
        pending: &Pending,
    ) -> CoreResult<crate::users::Session> {
        let cfg = self.stored_auth(app).await?;
        let p = cfg
            .enabled(provider)
            .ok_or(CoreError::NotFound("sign-in provider"))?;
        let e = self.endpoints(p).await?;
        let mut form = vec![
            ("grant_type", "authorization_code".to_owned()),
            ("code", code.to_owned()),
            ("redirect_uri", pending.callback.clone()),
            ("code_verifier", pending.verifier.clone()),
        ];
        let mut req = self
            .wh
            .client()
            .post(&e.token)
            .header("accept", "application/json");
        if e.basic {
            req = req.basic_auth(&p.client_id, Some(&p.client_secret));
        } else {
            form.push(("client_id", p.client_id.clone()));
            form.push(("client_secret", p.client_secret.clone()));
        }
        let tok: J = req
            .timeout(Duration::from_secs(10))
            .form(&form)
            .send()
            .await
            .map_err(|e| bad(format!("token request failed: {}", e.without_url())))?
            .json()
            .await
            .map_err(|_| bad("the provider's token response was not JSON"))?;
        let access = s(&tok, "access_token").ok_or_else(|| {
            bad(format!(
                "the provider refused the code{}",
                s(&tok, "error")
                    .map(|e| format!(" ({e})"))
                    .unwrap_or_default()
            ))
        })?;
        let profile = self.profile(provider, &e, &access, &p.client_id).await?;
        let id = self.identity_user(app, provider, &profile).await?;
        self.issue_session(app, &id).await
    }

    async fn profile(
        &self,
        provider: &str,
        e: &Endpoints,
        access: &str,
        client_id: &str,
    ) -> CoreResult<Profile> {
        let get = |url: &str| {
            let mut r = self
                .wh
                .client()
                .get(url)
                .bearer_auth(access)
                .header("accept", "application/json")
                .timeout(Duration::from_secs(10));
            if provider == "twitch" {
                r = r.header("client-id", client_id);
            }
            r
        };
        let fetch = |url: String| async move {
            get(&url)
                .send()
                .await
                .and_then(|r| r.error_for_status())
                .map_err(|e| bad(format!("profile request failed: {}", e.without_url())))?
                .json::<J>()
                .await
                .map_err(|_| bad("the provider's profile response was not JSON"))
        };
        let u = fetch(e.userinfo.clone()).await?;
        let none = || bad("the provider returned no user id");
        Ok(match provider {
            "github" => {
                // The public profile email may be hidden or unverified: ask for the primary verified one.
                let emails = fetch("https://api.github.com/user/emails".into())
                    .await
                    .ok();
                let primary = emails
                    .as_ref()
                    .and_then(|l| l.as_array())
                    .and_then(|l| {
                        l.iter()
                            .find(|x| x["primary"] == true && x["verified"] == true)
                    })
                    .and_then(|x| s(x, "email"));
                Profile {
                    subject: s(&u, "id").ok_or_else(none)?,
                    verified: primary.is_some(),
                    email: primary.or_else(|| s(&u, "email")),
                    name: s(&u, "name").or_else(|| s(&u, "login")).unwrap_or_default(),
                }
            }
            "discord" => Profile {
                subject: s(&u, "id").ok_or_else(none)?,
                verified: u["verified"] == true,
                email: s(&u, "email"),
                name: s(&u, "global_name")
                    .or_else(|| s(&u, "username"))
                    .unwrap_or_default(),
            },
            "facebook" => Profile {
                subject: s(&u, "id").ok_or_else(none)?,
                verified: false,
                email: s(&u, "email"),
                name: s(&u, "name").unwrap_or_default(),
            },
            "twitch" => {
                let d = &u["data"][0];
                Profile {
                    subject: s(d, "id").ok_or_else(none)?,
                    verified: false,
                    email: s(d, "email"),
                    name: s(d, "display_name").unwrap_or_default(),
                }
            }
            "x" => {
                let d = &u["data"];
                Profile {
                    subject: s(d, "id").ok_or_else(none)?,
                    verified: false,
                    email: None,
                    name: s(d, "name")
                        .or_else(|| s(d, "username"))
                        .unwrap_or_default(),
                }
            }
            // OIDC userinfo: google, microsoft, gitlab, linkedin, generic oidc.
            _ => Profile {
                subject: s(&u, "sub").ok_or_else(none)?,
                verified: u["email_verified"] == true || u["email_verified"] == "true",
                email: s(&u, "email"),
                name: s(&u, "name").unwrap_or_default(),
            },
        })
    }

    /// The user for a provider identity: known identity, link by verified email, or new user.
    async fn identity_user(&self, app: &str, provider: &str, p: &Profile) -> CoreResult<Id> {
        let key = |sql: &str, params: Vec<Value>| Query {
            sql: sql.into(),
            params,
        };
        let known = self
            .sys
            .query(key(
                "SELECT i.user, u.disabled FROM app_identities i JOIN app_users u ON u.id = i.user \
                 WHERE i.app = ? AND i.provider = ? AND i.subject = ?",
                vec![
                    Value::Text(app.into()),
                    Value::Text(provider.into()),
                    Value::Text(p.subject.as_str().into()),
                ],
            ))
            .await?;
        if let Some(r) = known.first() {
            let (Value::Blob(id), Value::Int(disabled)) = (&r[0], &r[1]) else {
                return Err(CoreError::Internal("bad identity row".into()));
            };
            if *disabled != 0 {
                return Err(CoreError::Forbidden("this account is disabled"));
            }
            return <Id>::try_from(&id[..]).map_err(|_| CoreError::Internal("bad id".into()));
        }
        let email = p
            .email
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase);
        let existing = match &email {
            Some(e) => self
                .sys
                .query(key(
                    "SELECT id, disabled FROM app_users WHERE app = ? AND email = ?",
                    vec![Value::Text(app.into()), Value::Text(e.as_str().into())],
                ))
                .await?
                .first()
                .and_then(|r| match (&r[0], &r[1]) {
                    (Value::Blob(id), Value::Int(d)) => {
                        Some((<Id>::try_from(&id[..]).ok()?, *d != 0))
                    }
                    _ => None,
                }),
            None => None,
        };
        let id = match existing {
            Some((_, true)) => return Err(CoreError::Forbidden("this account is disabled")),
            Some((id, false)) if p.verified => id,
            Some(_) => {
                return Err(CoreError::Conflict(
                    "an account with this email exists; sign in with it first (the provider did not verify the email)".into(),
                ));
            }
            None => {
                self.insert_user(
                    app,
                    email.as_deref(),
                    None,
                    &p.name.chars().take(100).collect::<String>(),
                )
                .await?
            }
        };
        self.sys
            .write(sys_write(
                "INSERT OR IGNORE INTO app_identities (app, provider, subject, user) VALUES (?, ?, ?, ?)",
                [0; 16],
                vec![
                    Value::Text(app.into()),
                    Value::Text(provider.into()),
                    Value::Text(p.subject.as_str().into()),
                    Value::Blob(id.into()),
                ],
            ))
            .await?;
        Ok(id)
    }
}

fn enc(s: &str) -> String {
    reqwest::Url::parse_with_params("http://x/", [("k", s)])
        .map(|u| {
            u.query()
                .unwrap_or_default()
                .trim_start_matches("k=")
                .to_owned()
        })
        .unwrap_or_default()
}
