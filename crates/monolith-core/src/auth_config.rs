//! Per-app sign-in configuration (Firebase-style "Authentication → Sign-in method").
//!
//! Stored in `system.db` `settings` under `auth:<app>`. This is configuration only: token
//! issuance and the OAuth code exchange land with Phase 2 auth (doc 08). Client secrets are
//! write-only: they are never returned by the API.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::FieldError;
use crate::error::{CoreError, CoreResult};

pub const MIN_PASSWORD_LEN: u32 = 8;
pub const MAX_PASSWORD_LEN: u32 = 72;
const MAX_FIELD: usize = 256;
const MAX_SECRET: usize = 2048;

/// A sign-in provider the server knows how to configure. `fields` are provider-specific
/// extras beyond client id and secret; `required` lists the ones that must be set to enable it.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDef {
    pub id: &'static str,
    pub name: &'static str,
    pub fields: &'static [&'static str],
    pub required: &'static [&'static str],
}

const fn p(id: &'static str, name: &'static str) -> ProviderDef {
    ProviderDef {
        id,
        name,
        fields: &[],
        required: &[],
    }
}

pub const CATALOG: &[ProviderDef] = &[
    p("google", "Google"),
    p("github", "GitHub"),
    ProviderDef {
        id: "apple",
        name: "Apple",
        fields: &["teamId", "keyId"],
        required: &["teamId", "keyId"],
    },
    ProviderDef {
        id: "microsoft",
        name: "Microsoft",
        fields: &["tenant"],
        required: &[],
    },
    p("facebook", "Facebook"),
    p("discord", "Discord"),
    ProviderDef {
        id: "gitlab",
        name: "GitLab",
        fields: &["baseUrl"],
        required: &[],
    },
    p("x", "X (Twitter)"),
    p("linkedin", "LinkedIn"),
    p("twitch", "Twitch"),
    ProviderDef {
        id: "oidc",
        name: "OpenID Connect",
        fields: &["issuer"],
        required: &["issuer"],
    },
];

fn def(id: &str) -> Option<&'static ProviderDef> {
    CATALOG.iter().find(|d| d.id == id)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EmailPassword {
    pub enabled: bool,
    pub allow_signup: bool,
    pub min_password_length: u32,
}

impl Default for EmailPassword {
    fn default() -> Self {
        Self {
            enabled: true,
            allow_signup: true,
            min_password_length: MIN_PASSWORD_LEN,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredProvider {
    pub id: String,
    pub enabled: bool,
    pub client_id: String,
    pub client_secret: String,
    #[serde(default)]
    pub extra: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StoredAuth {
    pub email_password: EmailPassword,
    pub providers: Vec<StoredProvider>,
}

/// One provider as returned by the API: no secret, only whether one is set.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    pub id: String,
    pub name: &'static str,
    pub enabled: bool,
    pub client_id: String,
    pub client_secret_set: bool,
    pub extra: BTreeMap<String, String>,
    /// Register this path (on your public origin) as the OAuth redirect URI.
    pub callback_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfigView {
    pub email_password: EmailPassword,
    pub providers: Vec<ProviderView>,
    pub catalog: &'static [ProviderDef],
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderIn {
    pub id: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub client_id: String,
    /// Omitted keeps the stored secret; an empty string clears it.
    pub client_secret: Option<String>,
    #[serde(default)]
    pub extra: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthConfigIn {
    #[serde(default)]
    pub email_password: EmailPassword,
    #[serde(default)]
    pub providers: Vec<ProviderIn>,
}

impl StoredAuth {
    pub fn view(&self, app: &str) -> AuthConfigView {
        AuthConfigView {
            email_password: self.email_password.clone(),
            providers: self
                .providers
                .iter()
                .filter_map(|s| {
                    Some(ProviderView {
                        id: s.id.clone(),
                        name: def(&s.id)?.name,
                        enabled: s.enabled,
                        client_id: s.client_id.clone(),
                        client_secret_set: !s.client_secret.is_empty(),
                        extra: s.extra.clone(),
                        callback_path: format!("/api/v1/apps/{app}/auth/oauth/{}/callback", s.id),
                    })
                })
                .collect(),
            catalog: CATALOG,
        }
    }

    /// Validates `input` and merges it over `self` (secrets are kept when omitted).
    pub fn apply(&self, input: AuthConfigIn) -> CoreResult<StoredAuth> {
        let mut errs = Vec::new();
        let ep = &input.email_password;
        if !(MIN_PASSWORD_LEN..=MAX_PASSWORD_LEN).contains(&ep.min_password_length) {
            errs.push(FieldError::new(
                "emailPassword.minPasswordLength",
                "out_of_range",
                format!("must be within {MIN_PASSWORD_LEN}..={MAX_PASSWORD_LEN}"),
            ));
        }
        let mut out = Vec::with_capacity(input.providers.len());
        for (i, pin) in input.providers.iter().enumerate() {
            let at = |f: &str| format!("providers[{i}].{f}");
            let Some(d) = def(&pin.id) else {
                errs.push(FieldError::new(
                    &at("id"),
                    "unknown_provider",
                    format!("unknown provider '{}'", pin.id),
                ));
                continue;
            };
            if input.providers[..i].iter().any(|o| o.id == pin.id) {
                errs.push(FieldError::new(
                    &at("id"),
                    "duplicate",
                    "provider listed twice",
                ));
                continue;
            }
            let client_id = pin.client_id.trim().to_owned();
            if client_id.len() > MAX_FIELD {
                errs.push(FieldError::new(&at("clientId"), "too_long", "too long"));
            }
            let secret = match &pin.client_secret {
                Some(s) => s.trim().to_owned(),
                None => self
                    .providers
                    .iter()
                    .find(|o| o.id == pin.id)
                    .map(|o| o.client_secret.clone())
                    .unwrap_or_default(),
            };
            if secret.len() > MAX_SECRET {
                errs.push(FieldError::new(&at("clientSecret"), "too_long", "too long"));
            }
            let mut extra = BTreeMap::new();
            for (k, v) in &pin.extra {
                if !d.fields.contains(&k.as_str()) {
                    errs.push(FieldError::new(
                        &at(&format!("extra.{k}")),
                        "unknown_field",
                        format!("{} has no field '{k}'", d.name),
                    ));
                } else if v.trim().len() > MAX_FIELD {
                    errs.push(FieldError::new(
                        &at(&format!("extra.{k}")),
                        "too_long",
                        "too long",
                    ));
                } else if !v.trim().is_empty() {
                    extra.insert(k.clone(), v.trim().to_owned());
                }
            }
            if pin.enabled {
                if client_id.is_empty() {
                    errs.push(FieldError::new(
                        &at("clientId"),
                        "required",
                        "required to enable",
                    ));
                }
                if secret.is_empty() {
                    errs.push(FieldError::new(
                        &at("clientSecret"),
                        "required",
                        "required to enable",
                    ));
                }
                for r in d.required {
                    if !extra.contains_key(*r) {
                        errs.push(FieldError::new(
                            &at(&format!("extra.{r}")),
                            "required",
                            "required to enable",
                        ));
                    }
                }
            }
            for (k, v) in &extra {
                if matches!(k.as_str(), "issuer" | "baseUrl") && !v.starts_with("https://") {
                    errs.push(FieldError::new(
                        &at(&format!("extra.{k}")),
                        "invalid_format",
                        "must be an https:// URL",
                    ));
                }
            }
            out.push(StoredProvider {
                id: pin.id.clone(),
                enabled: pin.enabled,
                client_id,
                client_secret: secret,
                extra,
            });
        }
        if errs.is_empty() {
            Ok(StoredAuth {
                email_password: input.email_password,
                providers: out,
            })
        } else {
            Err(CoreError::Validation(errs))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(v: serde_json::Value) -> AuthConfigIn {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn secrets_are_kept_hidden_and_validated() {
        let base = StoredAuth::default();
        let a = base
            .apply(input(serde_json::json!({"providers":[
                {"id":"google","enabled":true,"clientId":"cid","clientSecret":"s3"},
                {"id":"github"}
            ]})))
            .unwrap();
        let v = serde_json::to_value(a.view("demo")).unwrap();
        assert_eq!(v["providers"][0]["clientSecretSet"], true);
        assert!(!v.to_string().contains("s3"));
        assert_eq!(
            v["providers"][0]["callbackPath"],
            "/api/v1/apps/demo/auth/oauth/google/callback"
        );
        // Omitting clientSecret keeps it; empty clears it.
        let b = a
            .apply(input(
                serde_json::json!({"providers":[{"id":"google","enabled":true,"clientId":"cid2"}]}),
            ))
            .unwrap();
        assert_eq!(b.providers[0].client_secret, "s3");
        let err = b
            .apply(input(serde_json::json!({"providers":[{"id":"google","enabled":true,"clientId":"x","clientSecret":""}]})))
            .unwrap_err();
        assert!(
            matches!(err, CoreError::Validation(e) if e[0].field == "providers[0].clientSecret")
        );
    }

    #[test]
    fn rejects_bad_input() {
        let base = StoredAuth::default();
        for bad in [
            serde_json::json!({"providers":[{"id":"nope"}]}),
            serde_json::json!({"providers":[{"id":"github"},{"id":"github"}]}),
            serde_json::json!({"providers":[{"id":"github","extra":{"x":"y"}}]}),
            serde_json::json!({"providers":[{"id":"apple","enabled":true,"clientId":"a","clientSecret":"b"}]}),
            serde_json::json!({"providers":[{"id":"oidc","clientId":"a","extra":{"issuer":"http://x"}}]}),
            serde_json::json!({"emailPassword":{"minPasswordLength":3}}),
        ] {
            assert!(
                matches!(
                    base.apply(input(bad.clone())),
                    Err(CoreError::Validation(_))
                ),
                "{bad}"
            );
        }
    }
}
