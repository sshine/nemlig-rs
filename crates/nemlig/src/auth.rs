//! Signing in, and keeping the five-minute bearer token alive.
//!
//! nemlig.com's login is five requests, and the first two are made as nobody: an
//! anti-forgery token and an anonymous bearer token, which together authorise the login
//! POST, which sets the cookies, after which both tokens are fetched again as the signed-in
//! account. Only the last pair is worth keeping.
//!
//! Afterwards the bearer token is the only thing that expires on a timescale that matters,
//! so it is renewed from the cookies alone. The password is needed once, not every five
//! minutes.

use chrono::{TimeDelta, Utc};
use reqwest::Method;
use reqwest::header::{self, HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};

use crate::client::{Backend, Client, Secret, WEB_APP_VERSION};
use crate::error::{AuthError, Error, Result, truncate};
use crate::session::Tokens;

/// What nemlig.com reports back about the account when it signs in.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct SignIn {
    /// Where the website would have sent the browser next. Its presence is what marks
    /// the login as accepted; nemlig.com answers a wrong password with 200 and no field.
    pub redirect_url: String,
    /// Whether the basket held before signing in was merged into the account's.
    #[serde(default)]
    pub merge_successful: bool,
    /// Whether the account's postcode differs from the one the session was browsing.
    #[serde(default)]
    pub zip_code_differs: bool,
    /// The delivery slot the account currently has, in nemlig.com's opaque format.
    #[serde(default)]
    pub timeslot_utc: Option<crate::types::TimeslotUtc>,
    /// Which delivery zone the account's address falls in.
    #[serde(default)]
    pub delivery_zone_id: Option<i64>,
    /// Whether this was the account's first ever sign-in.
    #[serde(default)]
    pub is_first_login: bool,
    /// Meal boxes the merge dropped from the basket.
    #[serde(default)]
    pub removed_meal_boxes: Vec<serde_json::Value>,
}

impl Client {
    /// Sign in, replacing whatever session the client held.
    ///
    /// Not normally needed: every call signs in on its own. Reach for this to check a
    /// password, or to see what the merge did to the basket.
    pub async fn sign_in(&self) -> Result<SignIn> {
        let _guard = self.auth_lock().await;
        self.login().await
    }

    /// Forget the session, on disk as well as in memory.
    pub async fn sign_out(&self) -> Result<()> {
        self.session().clear().await
    }

    /// The tokens to send with the next request, signing in or renewing as needed.
    pub(crate) async fn tokens(&self) -> Result<Tokens> {
        if let Some(tokens) = self.session().tokens().await
            && !tokens.is_stale()
        {
            return Ok(tokens);
        }

        let _guard = self.auth_lock().await;

        // Another task may have signed in while this one waited for the lock.
        if let Some(tokens) = self.session().tokens().await
            && !tokens.is_stale()
        {
            return Ok(tokens);
        }

        if self.session().tokens().await.is_none() {
            self.session().restore().await;
            if let Some(tokens) = self.session().tokens().await
                && !tokens.is_stale()
            {
                return Ok(tokens);
            }
        }

        // Only worth attempting while the cookies are still believed good. After a 401
        // they are not, and renewing would hand back an anonymous token that fails the
        // same way one request later.
        if self.session().cookies_trusted().await && self.session().tokens().await.is_some() {
            match self.renew().await {
                Ok(tokens) => return Ok(tokens),
                Err(error) => {
                    tracing::debug!(%error, "renewing the session failed; signing in");
                }
            }
        }

        self.login().await?;
        // login() stores the tokens it just fetched, so this cannot be empty; the error
        // exists so a future change to that order fails loudly rather than silently.
        self.session()
            .tokens()
            .await
            .ok_or(Error::Auth(AuthError::Handshake {
                step: "login",
                missing: "token",
            }))
    }

    /// The five-request handshake.
    async fn login(&self) -> Result<SignIn> {
        self.session().reset().await;
        self.forget_context().await;

        let anonymous_xsrf = self.anti_forgery(None).await?;
        let (anonymous_bearer, _) = self.token(None).await?;

        let body = LoginRequest {
            username: self.credentials().username(),
            password: self.credentials().password.expose(),
            // The captured traffic's own flags. Merging matters because the handshake
            // starts from an empty cookie jar: without it the account's basket would be
            // replaced by the anonymous one this client just created, which is empty.
            check_for_existing_products: true,
            do_merge: true,
            app_installed: false,
            save_existing_basket: false,
        };

        let outcome: SignIn = self
            .handshake(
                Method::POST,
                &["webapi", "login"],
                Some(&anonymous_bearer),
                Some(&anonymous_xsrf),
                "login?returnUrl=%2F",
                Some(serde_json::to_value(&body).map_err(Error::Encode)?),
            )
            .await
            .and_then(|raw| parse(&raw, "the login response"))
            .map_err(|error| match error {
                // A wrong password is a 200 whose body has no RedirectUrl, so the decode
                // failure is the rejection rather than a surprise.
                Error::Decode { .. } => Error::Auth(AuthError::Rejected),
                other => other,
            })?;

        let (bearer, expires_in) = self.token(Some(&anonymous_bearer)).await?;
        let xsrf = self.anti_forgery(Some(&bearer)).await?;

        self.session()
            .store(Tokens {
                bearer: Secret::new(bearer),
                expires_at: Utc::now() + expires_in,
                xsrf,
            })
            .await?;
        self.session().trust_cookies().await;

        tracing::info!(username = %self.credentials().username(), "signed in");
        Ok(outcome)
    }

    /// Renew the bearer and anti-forgery tokens from the cookies, without the password.
    async fn renew(&self) -> Result<Tokens> {
        let (bearer, expires_in) = self.token(None).await?;
        let xsrf = self.anti_forgery(Some(&bearer)).await?;
        let tokens = Tokens {
            bearer: Secret::new(bearer),
            expires_at: Utc::now() + expires_in,
            xsrf,
        };
        self.session().store(tokens.clone()).await?;
        tracing::debug!("renewed the session token");
        Ok(tokens)
    }

    /// `GET /webapi/AntiForgery`, which answers with the token and the header to put it in.
    async fn anti_forgery(&self, bearer: Option<&str>) -> Result<String> {
        #[derive(Deserialize)]
        struct AntiForgery {
            #[serde(rename = "Value")]
            value: String,
        }

        let raw = self
            .handshake(
                Method::GET,
                &["webapi", "AntiForgery"],
                bearer,
                None,
                "",
                None,
            )
            .await?;
        let parsed: AntiForgery = parse(&raw, "the anti-forgery token").map_err(|_| {
            Error::Auth(AuthError::Handshake {
                step: "anti-forgery",
                missing: "token",
            })
        })?;
        Ok(parsed.value)
    }

    /// `GET /webapi/Token`, which mints a bearer token for whoever the cookies say you are.
    async fn token(&self, bearer: Option<&str>) -> Result<(String, TimeDelta)> {
        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
            #[serde(default)]
            expires_in: i64,
        }

        let raw = self
            .handshake(Method::GET, &["webapi", "Token"], bearer, None, "", None)
            .await?;
        let parsed: TokenResponse = parse(&raw, "the access token").map_err(|_| {
            Error::Auth(AuthError::Handshake {
                step: "token",
                missing: "token",
            })
        })?;

        // A token with no stated lifetime is treated as already due for renewal rather
        // than as eternal, which costs one extra request and never an unexplained 401.
        let lifetime = TimeDelta::try_seconds(parsed.expires_in).unwrap_or(TimeDelta::zero());
        Ok((parsed.access_token, lifetime))
    }

    /// One request of the handshake, which cannot go through the normal pipeline because
    /// that pipeline is what calls this to get its tokens.
    async fn handshake(
        &self,
        method: Method,
        segments: &[&str],
        bearer: Option<&str>,
        xsrf: Option<&str>,
        referer: &str,
        body: Option<serde_json::Value>,
    ) -> Result<String> {
        let url = self.url(Backend::Site, segments)?;
        let context = format!("{method} {}", url.path());

        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static("application/json, text/plain, */*"),
        );
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        headers.insert(
            HeaderName::from_static("device-size"),
            HeaderValue::from_static("desktop"),
        );
        headers.insert(
            HeaderName::from_static("platform"),
            HeaderValue::from_static("web"),
        );
        headers.insert(
            HeaderName::from_static("version"),
            HeaderValue::from_static(WEB_APP_VERSION),
        );
        headers.insert(
            HeaderName::from_static("x-correlation-id"),
            HeaderValue::from_str(&uuid::Uuid::new_v4().to_string())
                .unwrap_or(HeaderValue::from_static("")),
        );
        if let Some(bearer) = bearer {
            let mut value = HeaderValue::from_str(&format!("Bearer {bearer}")).map_err(|_| {
                Error::Auth(AuthError::Handshake {
                    step: "token",
                    missing: "token",
                })
            })?;
            value.set_sensitive(true);
            headers.insert(header::AUTHORIZATION, value);
        }
        if let Some(xsrf) = xsrf {
            headers.insert(
                HeaderName::from_static("x-xsrf-token"),
                HeaderValue::from_str(xsrf).unwrap_or(HeaderValue::from_static("")),
            );
        }
        if let Ok(referer) = self.site_url(referer)
            && let Ok(value) = HeaderValue::from_str(referer.as_str())
        {
            headers.insert(header::REFERER, value);
        }

        let mut request = self
            .http()
            .request(method.clone(), url.clone())
            .headers(headers);
        if let Some(body) = body {
            request = request.json(&body);
        }

        self.pace().await;
        self.count_request();
        let response = request
            .send()
            .await
            .map_err(|source| Error::transport(context.clone(), source))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|source| Error::transport(format!("reading {context}"), source))?;

        if !status.is_success() {
            return Err(Error::Api {
                status: status.as_u16(),
                method: if method == Method::POST {
                    "POST"
                } else {
                    "GET"
                },
                path: url.path().to_owned(),
                body: truncate(&text),
            });
        }
        Ok(text)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct LoginRequest<'a> {
    username: &'a str,
    password: &'a str,
    check_for_existing_products: bool,
    do_merge: bool,
    app_installed: bool,
    save_existing_basket: bool,
}

fn parse<T: serde::de::DeserializeOwned>(raw: &str, expected: &'static str) -> Result<T> {
    serde_json::from_str(raw).map_err(|source| Error::Decode {
        expected,
        body: truncate(raw),
        source,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn the_login_body_keeps_the_casing_the_site_expects() {
        let body = LoginRequest {
            username: "someone@example.com",
            password: "hunter2-not-a-real-password",
            check_for_existing_products: true,
            do_merge: true,
            app_installed: false,
            save_existing_basket: false,
        };
        let json = serde_json::to_value(&body).expect("serializes");
        for key in [
            "Username",
            "Password",
            "CheckForExistingProducts",
            "DoMerge",
            "AppInstalled",
            "SaveExistingBasket",
        ] {
            assert!(json.get(key).is_some(), "{key} is missing");
        }
    }

    #[test]
    fn a_sign_in_without_a_redirect_is_not_a_sign_in() {
        assert!(serde_json::from_str::<SignIn>(r#"{"MergeSuccessful":true}"#).is_err());
        let ok: SignIn = serde_json::from_str(r#"{"RedirectUrl":"/","DeliveryZoneId":1}"#)
            .expect("a redirect is enough");
        assert_eq!(ok.redirect_url, "/");
        assert_eq!(ok.delivery_zone_id, Some(1));
    }
}
