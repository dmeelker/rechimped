//! Client for TimeChimp's internal web API (`web.timechimp.com/api`), as used by app.timechimp.com.
//!
//! This API is undocumented and may change without notice.
//!
//! ```no_run
//! # async fn demo() -> timechimp::Result<()> {
//! use timechimp::{Authenticator, Client, LoginOutcome};
//!
//! let auth = match Authenticator::login("me@example.com", "secret").await? {
//!     LoginOutcome::LoggedIn(auth) => auth,
//!     LoginOutcome::MfaRequired(challenge) => challenge.respond("123456").await?,
//! };
//! // Persist auth.refresh_token() to skip the password next time.
//! let client = Client::new(auth);
//! let me = client.current_user().await?;
//! # Ok(()) }
//! ```

mod auth;
mod client;
mod error;
pub mod models;
mod srp;

pub use auth::{Authenticator, CLIENT_ID, LoginOutcome, MfaChallenge, MfaKind, USER_POOL_ID};
pub use client::Client;
pub use error::{Error, Result};
pub use models::*;
