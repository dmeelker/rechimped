use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::error::{Error, Result};
use crate::srp::{self, ChallengeParams, SrpClient};

/// TimeChimp's Cognito user pool.
pub const USER_POOL_ID: &str = "eu-central-1_NIGD97pvM";
/// TimeChimp's public Cognito app client (no client secret).
pub const CLIENT_ID: &str = "7nq87ee95nibpha7loi8pb7lnp";

const COGNITO_URL: &str = "https://cognito-idp.eu-central-1.amazonaws.com/";
const REFRESH_MARGIN: Duration = Duration::from_secs(5 * 60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Per request, including reading the body; generous because report pages can be slow.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// The HTTP client shared by an `Authenticator` and its `Client`. With timeouts, so a hung
/// connection fails instead of keeping a request (and the app's busy state) open forever.
fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .expect("the HTTP client builder has no failing options")
}

/// Obtains and refreshes access tokens for the TimeChimp API.
pub struct Authenticator {
    /// Shared with the `Client` built on it, so both use one connection pool.
    pub(crate) http: reqwest::Client,
    /// `COGNITO_URL`, or a test server.
    cognito_url: String,
    refresh_token: String,
    access: Mutex<Option<AccessToken>>,
}

struct AccessToken {
    token: String,
    expires_at: Instant,
}

pub enum LoginOutcome {
    LoggedIn(Authenticator),
    MfaRequired(MfaChallenge),
}

/// A pending 2FA challenge after a successful password check.
pub struct MfaChallenge {
    http: reqwest::Client,
    cognito_url: String,
    challenge_name: String,
    session: String,
    username: String,
}

impl Authenticator {
    /// Logs in with email and password using SRP (the password is never sent).
    pub async fn login(email: &str, password: &str) -> Result<LoginOutcome> {
        Self::login_at(COGNITO_URL, email, password).await
    }

    /// [`Authenticator::login`] against another Cognito endpoint than AWS's, for tests.
    #[doc(hidden)]
    pub async fn login_at(cognito_url: &str, email: &str, password: &str) -> Result<LoginOutcome> {
        let http = http_client();
        let srp = SrpClient::new();

        let init = cognito(
            &http,
            cognito_url,
            "InitiateAuth",
            json!({
                "AuthFlow": "USER_SRP_AUTH",
                "ClientId": CLIENT_ID,
                "AuthParameters": { "USERNAME": email, "SRP_A": srp.public_key_hex() },
            }),
        )
        .await?;

        if init["ChallengeName"] != "PASSWORD_VERIFIER" {
            return Err(Error::Auth(format!("unexpected challenge: {}", init["ChallengeName"])));
        }
        let params = &init["ChallengeParameters"];
        let param = |key: &str| {
            params[key]
                .as_str()
                .ok_or_else(|| Error::Auth(format!("challenge is missing {key}")))
        };

        let timestamp = srp::timestamp(chrono::Utc::now());
        let secret_block = param("SECRET_BLOCK")?;
        let signature = srp
            .password_claim_signature(&ChallengeParams {
                pool_name: USER_POOL_ID.split_once('_').expect("pool id has region prefix").1,
                user_id: param("USER_ID_FOR_SRP")?,
                password,
                salt_hex: param("SALT")?,
                srp_b_hex: param("SRP_B")?,
                secret_block_b64: secret_block,
                timestamp: &timestamp,
            })
            .map_err(Error::Auth)?;

        let mut request = json!({
            "ChallengeName": "PASSWORD_VERIFIER",
            "ClientId": CLIENT_ID,
            "ChallengeResponses": {
                "USERNAME": param("USERNAME")?,
                "PASSWORD_CLAIM_SECRET_BLOCK": secret_block,
                "PASSWORD_CLAIM_SIGNATURE": signature,
                "TIMESTAMP": timestamp,
            },
        });
        if let Some(session) = init["Session"].as_str() {
            request["Session"] = session.into();
        }
        let response = cognito(&http, cognito_url, "RespondToAuthChallenge", request).await?;

        match response["ChallengeName"].as_str() {
            None => Ok(LoginOutcome::LoggedIn(Self::from_auth_result(http, cognito_url, &response)?)),
            Some(name @ ("SOFTWARE_TOKEN_MFA" | "SMS_MFA")) => Ok(LoginOutcome::MfaRequired(MfaChallenge {
                cognito_url: cognito_url.to_owned(),
                challenge_name: name.to_owned(),
                session: response["Session"].as_str().unwrap_or_default().to_owned(),
                username: response["ChallengeParameters"]["USER_ID_FOR_SRP"]
                    .as_str()
                    .or(params["USERNAME"].as_str())
                    .unwrap_or_default()
                    .to_owned(),
                http,
            })),
            Some(other) => Err(Error::Auth(format!("unsupported challenge: {other}"))),
        }
    }

    /// Resumes a session from a refresh token obtained earlier via [`Authenticator::refresh_token`].
    pub fn from_refresh_token(refresh_token: impl Into<String>) -> Self {
        Self::from_refresh_token_at(COGNITO_URL, refresh_token)
    }

    /// [`Authenticator::from_refresh_token`] against another Cognito endpoint, for tests.
    #[doc(hidden)]
    pub fn from_refresh_token_at(cognito_url: &str, refresh_token: impl Into<String>) -> Self {
        Self {
            http: http_client(),
            cognito_url: cognito_url.to_owned(),
            refresh_token: refresh_token.into(),
            access: Mutex::new(None),
        }
    }

    /// Uses `token` as the access token for an hour, without a refresh token, for tests against
    /// a server that takes any bearer token.
    #[doc(hidden)]
    pub fn with_access_token(token: impl Into<String>) -> Self {
        let token = AccessToken { token: token.into(), expires_at: Instant::now() + Duration::from_secs(3600) };
        Self {
            http: http_client(),
            cognito_url: COGNITO_URL.to_owned(),
            refresh_token: String::new(),
            access: Mutex::new(Some(token)),
        }
    }

    /// Long-lived token; persist it to avoid logging in with a password every time.
    pub fn refresh_token(&self) -> &str {
        &self.refresh_token
    }

    /// Returns a valid access token, refreshing it when it is (nearly) expired.
    pub async fn access_token(&self) -> Result<String> {
        let mut access = self.access.lock().await;
        if let Some(t) = access.as_ref().filter(|t| Instant::now() + REFRESH_MARGIN < t.expires_at) {
            return Ok(t.token.clone());
        }
        self.refresh(&mut access).await
    }

    /// A new access token after the server turned down `rejected`. When requests in parallel
    /// all get a 401, only the first refreshes; the others get the token it fetched.
    pub(crate) async fn refresh_after_rejection(&self, rejected: &str) -> Result<String> {
        let mut access = self.access.lock().await;
        if let Some(t) = access.as_ref().filter(|t| t.token != rejected) {
            return Ok(t.token.clone());
        }
        self.refresh(&mut access).await
    }

    async fn refresh(&self, access: &mut Option<AccessToken>) -> Result<String> {
        let response = cognito(
            &self.http,
            &self.cognito_url,
            "InitiateAuth",
            json!({
                "AuthFlow": "REFRESH_TOKEN_AUTH",
                "ClientId": CLIENT_ID,
                "AuthParameters": { "REFRESH_TOKEN": self.refresh_token },
            }),
        )
        .await?;
        let token = parse_access_token(&response)?;
        let value = token.token.clone();
        *access = Some(token);
        Ok(value)
    }

    fn from_auth_result(http: reqwest::Client, cognito_url: &str, response: &Value) -> Result<Self> {
        let refresh_token = response["AuthenticationResult"]["RefreshToken"]
            .as_str()
            .ok_or_else(|| Error::Auth("no refresh token in response".into()))?
            .to_owned();
        Ok(Self {
            http,
            cognito_url: cognito_url.to_owned(),
            refresh_token,
            access: Mutex::new(Some(parse_access_token(response)?)),
        })
    }
}

/// The kind of code an [`MfaChallenge`] asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MfaKind {
    /// Sent by SMS (`SMS_MFA`).
    Sms,
    /// From an authenticator app (`SOFTWARE_TOKEN_MFA`).
    Totp,
}

impl MfaChallenge {
    /// Where the code comes from.
    pub fn kind(&self) -> MfaKind {
        if self.challenge_name == "SMS_MFA" { MfaKind::Sms } else { MfaKind::Totp }
    }

    pub async fn respond(self, code: &str) -> Result<Authenticator> {
        let code_key = format!("{}_CODE", self.challenge_name);
        let response = cognito(
            &self.http,
            &self.cognito_url,
            "RespondToAuthChallenge",
            json!({
                "ChallengeName": self.challenge_name,
                "ClientId": CLIENT_ID,
                "Session": self.session,
                "ChallengeResponses": { "USERNAME": self.username, code_key: code },
            }),
        )
        .await?;
        Authenticator::from_auth_result(self.http, &self.cognito_url, &response)
    }
}

fn parse_access_token(response: &Value) -> Result<AccessToken> {
    let result = &response["AuthenticationResult"];
    let token = result["AccessToken"]
        .as_str()
        .ok_or_else(|| Error::Auth("no access token in response".into()))?;
    let expires_in = result["ExpiresIn"].as_u64().unwrap_or(3600);
    Ok(AccessToken {
        token: token.to_owned(),
        expires_at: Instant::now() + Duration::from_secs(expires_in),
    })
}

async fn cognito(http: &reqwest::Client, url: &str, target: &str, body: Value) -> Result<Value> {
    let response = http
        .post(url)
        .header("Content-Type", "application/x-amz-json-1.1")
        .header("X-Amz-Target", format!("AWSCognitoIdentityProviderService.{target}"))
        .body(body.to_string())
        .send()
        .await?;
    let status = response.status();
    let bytes = response.bytes().await?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|source| Error::Decode {
        url: url.into(),
        source,
    })?;
    if !status.is_success() {
        let kind = value["__type"].as_str().unwrap_or("UnknownError");
        let kind = kind.rsplit('#').next().unwrap_or(kind);
        let message = value["message"].as_str().unwrap_or_default();
        return Err(Error::Rejected { kind: kind.to_owned(), message: message.to_owned() });
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_token_refreshed_meanwhile_is_reused() {
        // The cached token differs from the rejected one, so another request already refreshed:
        // no request to Cognito (which would fail here, with no refresh token).
        let auth = Authenticator::with_access_token("new");
        assert_eq!(auth.refresh_after_rejection("old").await.unwrap(), "new");
    }
}
