use reqwest::StatusCode;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("TimeChimp API returned {status} for {url}: {body}")]
    Api { status: StatusCode, url: String, body: String },

    #[error("could not parse response from {url}: {source}")]
    Decode { url: String, source: serde_json::Error },

    #[error("authentication failed: {0}")]
    Auth(String),
}

pub type Result<T> = std::result::Result<T, Error>;
