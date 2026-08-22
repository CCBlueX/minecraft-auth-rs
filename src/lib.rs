//! Microsoft, Xbox Live and Minecraft Java Edition authentication.
//!
//! This crate currently covers the Java Edition SISU login chain (device
//! code and webview-adapter sign-in). See `README.md` for scope not yet
//! covered (Bedrock, PlayFab, Realms, Xbox profile).

mod clock;
mod crypto;
mod error;
mod expirable;

/// Applied to every request this crate makes, on top of whatever the caller
/// configured on the `reqwest::Client` it handed us. All of these endpoints
/// exchange small JSON payloads, so a request still in flight after this
/// long is stuck, not slow — and without a bound it would hang the caller
/// forever instead of surfacing an error it can show or retry.
pub(crate) const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub mod java;
pub mod msa;
pub mod xbl;

pub use crypto::EcKeyPair;
pub use error::{Error, Result};
pub use expirable::Expirable;
