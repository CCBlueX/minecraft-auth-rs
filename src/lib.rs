//! Microsoft, Xbox Live and Minecraft Java Edition authentication.
//!
//! This crate currently covers the Java Edition SISU login chain (device
//! code and webview-adapter sign-in). See `README.md` for scope not yet
//! covered (Bedrock, PlayFab, Realms, Xbox profile).

mod clock;
mod crypto;
mod error;
mod expirable;

pub mod java;
pub mod msa;
pub mod xbl;

pub use crypto::EcKeyPair;
pub use error::{Error, Result};
pub use expirable::Expirable;
