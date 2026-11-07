//! App end users (doc 08): email + password accounts, linked OAuth identities, access and
//! refresh tokens. Users live in system.db (`app_users`, `app_identities`, `app_sessions`)
//! next to the per-app sign-in configuration.
//!
//! Access tokens are signed and carry app + user id, so verifying one needs no DB read
//! (15 minutes). Refresh tokens are 256-bit random strings, stored as SHA-256, valid 30 days,
//! rotated on every use. A disabled or deleted user loses refresh at once and the access
//! token when it expires. Rules see the user as `@request.auth.id` (collection `users`).
//!
//! Not yet: email verification, password reset, OTP/magic links, MFA, anonymous users,
//! per-IP rate limiting of failed logins, user records as a collection with custom fields.

use monolith_auth::{
    base64url, hash_password, random_bytes, sha256, sign_user_token, verify_password,
    verify_user_token,
};
use monolith_rules::Auth;
use monolith_store::id::{decode, encode_string, new_id};
use monolith_store::{AppStore, Id, Query, Value};
use serde::{Deserialize, Serialize};

use crate::app::{Principal, now_us};
use crate::auth_config::{MAX_PASSWORD_LEN, def};
use crate::codec::format_us;
use crate::error::{CoreError, CoreResult, FieldError};
use crate::system::{Registry, sys_delete, sys_write};

pub const ACCESS_TTL_US: i64 = 15 * 60 * 1_000_000;
pub const REFRESH_TTL_US: i64 = 30 * 86_400 * 1_000_000;
const MAX_NAME: usize = 100;
const MAX_EMAIL: usize = 254;

/// What a sign-in screen needs, readable without credentials.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicAuth {
    pub app: String,
    pub email_password: PublicEmail,
    pub providers: Vec<PublicProvider>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicEmail {
    pub enabled: bool,
    pub allow_signup: bool,
    pub min_password_length: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicProvider {
    pub id: String,
    pub name: &'static str,
    /// Open this (relative to the server) with `?redirectUri=` to start the sign-in.
    pub start_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInfo {
    pub id: String,
    pub email: Option<String>,
    pub name: String,
    pub created: String,
    pub disabled: bool,
    pub last_login: Option<String>,
    /// Sign-in methods: `password` and linked provider ids.
    pub providers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    /// Access token (`Authorization: Bearer …`), valid 15 minutes.
    pub token: String,
    pub refresh_token: String,
    pub expires_at: String,
    pub user: UserInfo,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegisterIn {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserEdit {
    pub name: Option<String>,
    pub disabled: Option<bool>,
    pub password: Option<String>,
}

fn invalid(field: &str, code: &'static str, msg: impl Into<String>) -> CoreError {
    CoreError::Validation(vec![FieldError::new(field, code, msg)])
}

fn blocking(e: tokio::task::JoinError) -> CoreError {
    CoreError::Internal(e.to_string())
}

fn norm_email(e: &str) -> CoreResult<String> {
    let e = e.trim().to_ascii_lowercase();
    let ok = e.len() <= MAX_EMAIL
        && !e.chars().any(char::is_whitespace)
        && e.split_once('@').is_some_and(|(l, d)| {
            !l.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.')
        });
    if ok {
        Ok(e)
    } else {
        Err(invalid(
            "email",
            "invalid_format",
            "enter a valid email address",
        ))
    }
}

fn check_name(n: &str) -> CoreResult<String> {
    let n = n.trim();
    if n.chars().count() > MAX_NAME {
        return Err(invalid(
            "name",
            "too_long",
            format!("at most {MAX_NAME} characters"),
        ));
    }
    Ok(n.to_owned())
}

fn check_password(p: &str, min: u32) -> CoreResult<()> {
    let n = p.chars().count();
    if n < min as usize || p.len() > MAX_PASSWORD_LEN as usize {
        return Err(invalid(
            "password",
            "out_of_range",
            format!("{min} to {MAX_PASSWORD_LEN} characters"),
        ));
    }
    Ok(())
}

const USER_COLS: &str = "id, email, name, created, disabled, last_login, \
    (SELECT group_concat(provider) FROM app_identities i WHERE i.user = app_users.id), hash IS NOT NULL";

fn user_from(r: &[Value]) -> Option<UserInfo> {
    let Value::Blob(id) = &r[0] else {
        return None;
    };
    let id: Id = <Id>::try_from(&id[..]).ok()?;
    let mut providers: Vec<String> = match &r[6] {
        Value::Text(t) => t.split(',').map(str::to_owned).collect(),
        _ => Vec::new(),
    };
    if matches!(r[7], Value::Int(1)) {
        providers.insert(0, "password".into());
    }
    Some(UserInfo {
        id: encode_string(&id),
        email: match &r[1] {
            Value::Text(t) => Some(t.to_string()),
            _ => None,
        },
        name: match &r[2] {
            Value::Text(t) => t.to_string(),
            _ => String::new(),
        },
        created: format_us(match &r[3] {
            Value::Int(n) => *n,
            _ => 0,
        }),
        disabled: matches!(r[4], Value::Int(1)),
        last_login: match &r[5] {
            Value::Int(n) => Some(format_us(*n)),
            _ => None,
        },
        providers,
    })
}

impl Registry {
    /// Sign-in methods this app offers; only what actually works is listed.
    pub async fn public_auth(&self, app: &str) -> CoreResult<PublicAuth> {
        self.app_info(app).await?;
        let cfg = self.stored_auth(app).await?;
        let ep = &cfg.email_password;
        Ok(PublicAuth {
            app: app.to_owned(),
            email_password: PublicEmail {
                enabled: ep.enabled,
                allow_signup: ep.enabled && ep.allow_signup,
                min_password_length: ep.min_password_length,
            },
            providers: cfg
                .providers
                .iter()
                .filter(|p| p.enabled && crate::oauth::supported(&p.id))
                .filter_map(|p| {
                    let d = def(&p.id)?;
                    Some(PublicProvider {
                        id: p.id.clone(),
                        name: d.name,
                        start_path: format!("/api/v1/apps/{app}/auth/oauth/{}/start", p.id),
                    })
                })
                .collect(),
        })
    }

    pub async fn register_user(&self, app: &str, input: RegisterIn) -> CoreResult<Session> {
        self.app_info(app).await?;
        let cfg = self.stored_auth(app).await?;
        if !cfg.email_password.enabled || !cfg.email_password.allow_signup {
            return Err(CoreError::Forbidden(
                "sign-up with email and password is disabled",
            ));
        }
        let email = norm_email(&input.email)?;
        check_password(&input.password, cfg.email_password.min_password_length)?;
        let name = check_name(&input.name)?;
        let pw = input.password;
        let hash = tokio::task::spawn_blocking(move || hash_password(&pw))
            .await
            .map_err(blocking)?
            .map_err(|e| CoreError::Internal(e.to_string()))?;
        let id = self
            .insert_user(app, Some(&email), Some(&hash), &name)
            .await?;
        self.issue_session(app, &id).await
    }

    /// Inserts a user row; a taken email is a 409.
    pub(crate) async fn insert_user(
        &self,
        app: &str,
        email: Option<&str>,
        hash: Option<&str>,
        name: &str,
    ) -> CoreResult<Id> {
        let id = new_id();
        self.sys
            .write(sys_write(
                "INSERT INTO app_users (id, app, email, hash, name, created, disabled) \
                 VALUES (?, ?, ?, ?, ?, ?, 0)",
                id,
                vec![
                    Value::Blob(id.into()),
                    Value::Text(app.into()),
                    email.map_or(Value::Null, |e| Value::Text(e.into())),
                    hash.map_or(Value::Null, |h| Value::Text(h.into())),
                    Value::Text(name.into()),
                    Value::Int(now_us()),
                ],
            ))
            .await
            .map_err(|e| match e {
                monolith_store::Error::Constraint(_) => {
                    CoreError::Conflict("an account with this email already exists".into())
                }
                e => e.into(),
            })?;
        Ok(id)
    }

    /// Wrong email, wrong password and disabled accounts all answer 401 after the same work.
    pub async fn sign_in_user(
        &self,
        app: &str,
        email: &str,
        password: &str,
    ) -> CoreResult<Session> {
        self.app_info(app).await?;
        let cfg = self.stored_auth(app).await?;
        if !cfg.email_password.enabled {
            return Err(CoreError::Forbidden(
                "sign-in with email and password is disabled",
            ));
        }
        let rows = self
            .sys
            .query(Query {
                sql: "SELECT id, hash, disabled FROM app_users WHERE app = ? AND email = ?".into(),
                params: vec![
                    Value::Text(app.into()),
                    Value::Text(email.trim().to_ascii_lowercase().into()),
                ],
            })
            .await?;
        let (id, hash, disabled) = match rows.first().map(|r| (&r[0], &r[1], &r[2])) {
            Some((Value::Blob(id), Value::Text(h), Value::Int(d))) => {
                (<Id>::try_from(&id[..]).ok(), h.to_string(), *d != 0)
            }
            _ => (None, self.dummy_hash.clone(), true),
        };
        let pw = password.to_owned();
        let ok = tokio::task::spawn_blocking(move || verify_password(&pw, &hash))
            .await
            .map_err(blocking)?;
        match (ok, id, disabled) {
            (true, Some(id), false) => self.issue_session(app, &id).await,
            _ => Err(CoreError::Unauthorized),
        }
    }

    /// Mints an access + refresh token pair and records the sign-in.
    pub(crate) async fn issue_session(&self, app: &str, id: &Id) -> CoreResult<Session> {
        let now = now_us();
        let refresh =
            base64url(&random_bytes::<32>().map_err(|e| CoreError::Internal(e.to_string()))?);
        let _ = self
            .sys
            .write(sys_delete(
                "DELETE FROM app_sessions WHERE exp < ?",
                vec![Value::Int(now)],
            ))
            .await;
        self.sys
            .write(sys_write(
                "INSERT INTO app_sessions (hash, app, user, exp) VALUES (?, ?, ?, ?)",
                [0; 16],
                vec![
                    Value::Blob(sha256(refresh.as_bytes()).to_vec().into()),
                    Value::Text(app.into()),
                    Value::Blob((*id).into()),
                    Value::Int(now + REFRESH_TTL_US),
                ],
            ))
            .await?;
        let _ = self
            .sys
            .write(sys_write(
                "UPDATE app_users SET last_login = ? WHERE id = ?",
                *id,
                vec![Value::Int(now), Value::Blob((*id).into())],
            ))
            .await;
        let exp = now + ACCESS_TTL_US;
        Ok(Session {
            token: sign_user_token(&self.token_key, app, id, exp),
            refresh_token: refresh,
            expires_at: format_us(exp),
            user: self.user_info(app, id).await?,
        })
    }

    pub async fn user_info(&self, app: &str, id: &Id) -> CoreResult<UserInfo> {
        self.sys
            .query(Query {
                sql: format!("SELECT {USER_COLS} FROM app_users WHERE id = ? AND app = ?").into(),
                params: vec![Value::Blob((*id).into()), Value::Text(app.into())],
            })
            .await?
            .first()
            .and_then(|r| user_from(r))
            .ok_or(CoreError::NotFound("user"))
    }

    /// Trades a refresh token for a new pair; the old token stops working.
    pub async fn refresh_session(&self, app: &str, refresh: &str) -> CoreResult<Session> {
        let h = sha256(refresh.as_bytes()).to_vec();
        let rows = self
            .sys
            .query(Query {
                sql: "SELECT s.user, s.exp, u.disabled FROM app_sessions s \
                      JOIN app_users u ON u.id = s.user WHERE s.hash = ? AND s.app = ?"
                    .into(),
                params: vec![Value::Blob(h.clone().into()), Value::Text(app.into())],
            })
            .await?;
        let Some((Value::Blob(user), Value::Int(exp), Value::Int(disabled))) =
            rows.first().map(|r| (&r[0], &r[1], &r[2]))
        else {
            return Err(CoreError::Unauthorized);
        };
        // Deleting first makes a replayed token fail even when two requests race.
        match self
            .sys
            .write(sys_delete(
                "DELETE FROM app_sessions WHERE hash = ?",
                vec![Value::Blob(h.into())],
            ))
            .await
        {
            Ok(_) => {}
            Err(monolith_store::Error::NotFound) => return Err(CoreError::Unauthorized),
            Err(e) => return Err(e.into()),
        }
        let id = <Id>::try_from(&user[..]).map_err(|_| CoreError::Unauthorized)?;
        if *exp <= now_us() || *disabled != 0 {
            return Err(CoreError::Unauthorized);
        }
        self.issue_session(app, &id).await
    }

    /// Ends one session; an unknown token is not an error (sign-out is idempotent).
    pub async fn sign_out_user(&self, app: &str, refresh: &str) -> CoreResult<()> {
        match self
            .sys
            .write(sys_delete(
                "DELETE FROM app_sessions WHERE hash = ? AND app = ?",
                vec![
                    Value::Blob(sha256(refresh.as_bytes()).to_vec().into()),
                    Value::Text(app.into()),
                ],
            ))
            .await
        {
            Ok(_) | Err(monolith_store::Error::NotFound) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// No DB access: signature, app and expiry only.
    pub fn verify_user(&self, app: &str, token: &str) -> Option<Principal> {
        let id = verify_user_token(&self.token_key, token, app, now_us())?;
        Some(Principal {
            superuser: false,
            auth: Some(Auth {
                id,
                collection: "users".into(),
                claims: Vec::new(),
            }),
        })
    }

    pub async fn update_me(&self, app: &str, id: &Id, name: &str) -> CoreResult<UserInfo> {
        let name = check_name(name)?;
        self.sys
            .write(sys_write(
                "UPDATE app_users SET name = ? WHERE id = ? AND app = ?",
                *id,
                vec![
                    Value::Text(name.into()),
                    Value::Blob((*id).into()),
                    Value::Text(app.into()),
                ],
            ))
            .await
            .map_err(|e| match e {
                monolith_store::Error::NotFound => CoreError::NotFound("user"),
                e => e.into(),
            })?;
        self.user_info(app, id).await
    }

    /// Needs the current password; every session of the user ends except the caller's next refresh.
    pub async fn change_password(
        &self,
        app: &str,
        id: &Id,
        old: &str,
        new: &str,
    ) -> CoreResult<()> {
        let cfg = self.stored_auth(app).await?;
        check_password(new, cfg.email_password.min_password_length)?;
        let rows = self
            .sys
            .query(Query {
                sql: "SELECT hash FROM app_users WHERE id = ? AND app = ?".into(),
                params: vec![Value::Blob((*id).into()), Value::Text(app.into())],
            })
            .await?;
        let hash = match rows.first().and_then(|r| r.first()) {
            Some(Value::Text(h)) => h.to_string(),
            Some(_) => {
                return Err(CoreError::BadRequest(
                    "this account has no password; it signs in with a provider".into(),
                ));
            }
            None => return Err(CoreError::NotFound("user")),
        };
        let old = old.to_owned();
        if !tokio::task::spawn_blocking(move || verify_password(&old, &hash))
            .await
            .map_err(blocking)?
        {
            return Err(CoreError::Unauthorized);
        }
        self.set_password(app, id, new).await
    }

    async fn set_password(&self, app: &str, id: &Id, password: &str) -> CoreResult<()> {
        let pw = password.to_owned();
        let hash = tokio::task::spawn_blocking(move || hash_password(&pw))
            .await
            .map_err(blocking)?
            .map_err(|e| CoreError::Internal(e.to_string()))?;
        self.sys
            .write(sys_write(
                "UPDATE app_users SET hash = ? WHERE id = ? AND app = ?",
                *id,
                vec![
                    Value::Text(hash.into()),
                    Value::Blob((*id).into()),
                    Value::Text(app.into()),
                ],
            ))
            .await?;
        self.revoke_sessions(id).await
    }

    async fn revoke_sessions(&self, id: &Id) -> CoreResult<()> {
        match self
            .sys
            .write(sys_delete(
                "DELETE FROM app_sessions WHERE user = ?",
                vec![Value::Blob((*id).into())],
            ))
            .await
        {
            Ok(_) | Err(monolith_store::Error::NotFound) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    // ---------- console (superuser) ----------

    /// Users of an app, oldest first; pass the last id as `after` for the next page.
    pub async fn list_users(
        &self,
        app: &str,
        limit: usize,
        after: Option<&str>,
    ) -> CoreResult<(Vec<UserInfo>, Option<String>)> {
        self.app_info(app).await?;
        let limit = limit.clamp(1, 200);
        let after = match after {
            Some(a) => decode(a).ok_or_else(|| CoreError::BadRequest("invalid `after`".into()))?,
            None => [0; 16],
        };
        let rows = self
            .sys
            .query(Query {
                sql: format!(
                    "SELECT {USER_COLS} FROM app_users WHERE app = ? AND id > ? ORDER BY id LIMIT ?"
                )
                .into(),
                params: vec![
                    Value::Text(app.into()),
                    Value::Blob(after.into()),
                    Value::Int(limit as i64 + 1),
                ],
            })
            .await?;
        let mut items: Vec<UserInfo> = rows.iter().filter_map(|r| user_from(r)).collect();
        let next = (items.len() > limit).then(|| {
            items.truncate(limit);
            items[limit - 1].id.clone()
        });
        Ok((items, next))
    }

    pub async fn create_user(&self, app: &str, input: RegisterIn) -> CoreResult<UserInfo> {
        self.app_info(app).await?;
        let cfg = self.stored_auth(app).await?;
        let email = norm_email(&input.email)?;
        check_password(&input.password, cfg.email_password.min_password_length)?;
        let name = check_name(&input.name)?;
        let pw = input.password;
        let hash = tokio::task::spawn_blocking(move || hash_password(&pw))
            .await
            .map_err(blocking)?
            .map_err(|e| CoreError::Internal(e.to_string()))?;
        let id = self
            .insert_user(app, Some(&email), Some(&hash), &name)
            .await?;
        self.user_info(app, &id).await
    }

    pub async fn update_user(&self, app: &str, id: &str, edit: UserEdit) -> CoreResult<UserInfo> {
        let id = decode(id).ok_or(CoreError::NotFound("user"))?;
        self.user_info(app, &id).await?;
        if let Some(n) = &edit.name {
            self.update_me(app, &id, n).await?;
        }
        if let Some(d) = edit.disabled {
            self.sys
                .write(sys_write(
                    "UPDATE app_users SET disabled = ? WHERE id = ? AND app = ?",
                    id,
                    vec![
                        Value::Int(i64::from(d)),
                        Value::Blob(id.into()),
                        Value::Text(app.into()),
                    ],
                ))
                .await?;
            if d {
                self.revoke_sessions(&id).await?;
            }
        }
        if let Some(p) = &edit.password {
            let cfg = self.stored_auth(app).await?;
            check_password(p, cfg.email_password.min_password_length)?;
            self.set_password(app, &id, p).await?;
        }
        self.user_info(app, &id).await
    }

    pub async fn delete_user(&self, app: &str, id: &str) -> CoreResult<()> {
        let id = decode(id).ok_or(CoreError::NotFound("user"))?;
        self.sys
            .write(sys_delete(
                "DELETE FROM app_users WHERE id = ? AND app = ?",
                vec![Value::Blob(id.into()), Value::Text(app.into())],
            ))
            .await
            .map_err(|e| match e {
                monolith_store::Error::NotFound => CoreError::NotFound("user"),
                e => e.into(),
            })?;
        for sql in [
            "DELETE FROM app_identities WHERE user = ?",
            "DELETE FROM app_sessions WHERE user = ?",
        ] {
            match self
                .sys
                .write(sys_delete(sql, vec![Value::Blob(id.into())]))
                .await
            {
                Ok(_) | Err(monolith_store::Error::NotFound) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    pub(crate) async fn drop_app_users(&self, app: &str) -> CoreResult<()> {
        for sql in [
            "DELETE FROM app_identities WHERE app = ?",
            "DELETE FROM app_sessions WHERE app = ?",
            "DELETE FROM app_users WHERE app = ?",
        ] {
            match self
                .sys
                .write(sys_delete(sql, vec![Value::Text(app.into())]))
                .await
            {
                Ok(_) | Err(monolith_store::Error::NotFound) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
}
