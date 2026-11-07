use reqwest::StatusCode;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("TimeChimp API returned {status} for {url}: {body}")]
    Api { status: StatusCode, url: String, body: String },

    #[error("could not parse response from {url}: {source}")]
    Decode { url: String, source: serde_json::Error },

    #[error("authentication failed: {kind}: {message}")]
    Rejected { kind: String, message: String },

    #[error("login service error: {kind}: {message}")]
    Cognito { kind: String, message: String },

    #[error("authentication failed: {0}")]
    Auth(String),

    #[error("{0}")]
    NotFound(String),
}

impl Error {
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
