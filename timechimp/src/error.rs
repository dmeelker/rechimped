use reqwest::StatusCode;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The request didn't get an answer (no network, DNS, timeout, ...).
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("TimeChimp API returned {status} for {url}: {body}")]
    Api { status: StatusCode, url: String, body: String },

    #[error("could not parse response from {url}: {source}")]
    Decode { url: String, source: serde_json::Error },

    /// Cognito turned the credentials down, e.g. `NotAuthorizedException` for a wrong password or
    /// a revoked or expired refresh token.
    #[error("authentication failed: {kind}: {message}")]
    Rejected { kind: String, message: String },

    /// Any other Cognito error, e.g. `TooManyRequestsException` or `InternalErrorException`:
    /// trouble on the way, not a verdict on the credentials.
    #[error("login service error: {kind}: {message}")]
    Cognito { kind: String, message: String },

    /// The login flow went somewhere this client doesn't handle (an unexpected challenge, a
    /// response without tokens).
    #[error("authentication failed: {0}")]
    Auth(String),

    /// A lookup found nothing, e.g. no route between two addresses.
    #[error("{0}")]
    NotFound(String),
}

impl Error {
    /// Whether the session itself is no good (logging in again is the fix), as opposed to a
    /// network or server problem that may pass.
    pub fn is_auth(&self) -> bool {
        match self {
            Error::Rejected { .. } | Error::Auth(_) => true,
            Error::Api { status, .. } => *status == StatusCode::UNAUTHORIZED,
            _ => false,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_errors() {
        let api = |status| Error::Api { status, url: String::new(), body: String::new() };
        assert!(Error::Rejected { kind: "NotAuthorizedException".into(), message: String::new() }.is_auth());
        assert!(Error::Auth("no access token in response".into()).is_auth());
        assert!(api(StatusCode::UNAUTHORIZED).is_auth());
        assert!(!api(StatusCode::INTERNAL_SERVER_ERROR).is_auth());
        assert!(!Error::NotFound("no route".into()).is_auth());
        assert!(!Error::Cognito { kind: "TooManyRequestsException".into(), message: String::new() }.is_auth());
    }
}
