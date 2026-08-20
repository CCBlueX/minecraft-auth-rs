use reqwest::StatusCode;
use thiserror::Error;

/// Crate-wide result type. Error variants never carry access tokens, refresh
/// tokens, authorization codes or private keys — only enough context to act
/// on or display the failure.
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("failed to parse response body: {0}")]
    Json(#[from] serde_json::Error),

    #[error("microsoft account error ({status}): {error}: {error_description}")]
    Msa {
        status: StatusCode,
        error: String,
        error_description: String,
    },

    #[error("xbox live error {error_code:#010x} ({name}): {message}")]
    Xbl {
        status: StatusCode,
        error_code: u64,
        name: String,
        message: String,
    },

    #[error("minecraft services error ({status}): {error}: {error_message}")]
    MinecraftServices {
        status: StatusCode,
        error: String,
        error_message: String,
    },

    #[error("this account has no Minecraft profile; the user must complete Minecraft setup at minecraft.net")]
    ProfileNotFound,

    #[error("received an unexpected response shape from {endpoint}: {reason}")]
    UnexpectedResponse {
        endpoint: &'static str,
        reason: String,
    },

    #[error("device code login timed out before the user entered the code")]
    DeviceCodeTimedOut,

    #[error("login was cancelled")]
    Cancelled,

    #[error("webview login failed: {0}")]
    Webview(String),

    #[error("{0}")]
    InvalidState(&'static str),
}
