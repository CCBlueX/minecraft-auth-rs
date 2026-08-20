use std::future::Future;
use std::time::Duration;

use reqwest::Client;
use tokio::time::{sleep, Instant};
use url::Url;

use crate::error::{Error, Result};
use crate::expirable::Expirable;

use super::config::MsaApplicationConfig;
use super::model::{MsaDeviceCode, MsaToken};
use super::request;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

/// Requests a device code, reports it to `on_code`, then polls until the
/// user finishes signing in at the verification URL or the code expires.
pub async fn login_with_device_code<F>(
    client: &Client,
    config: &MsaApplicationConfig,
    on_code: F,
) -> Result<MsaToken>
where
    F: FnOnce(&MsaDeviceCode),
{
    login_with_device_code_timeout(client, config, on_code, DEFAULT_TIMEOUT).await
}

pub async fn login_with_device_code_timeout<F>(
    client: &Client,
    config: &MsaApplicationConfig,
    on_code: F,
    timeout: Duration,
) -> Result<MsaToken>
where
    F: FnOnce(&MsaDeviceCode),
{
    let device_code = request::request_device_code(client, config).await?;
    on_code(&device_code);

    let deadline = Instant::now() + timeout;
    while !device_code.is_expired() && Instant::now() < deadline {
        match request::poll_device_code_token(client, config, &device_code.device_code).await {
            Ok(token) => return Ok(token),
            Err(Error::Msa { ref error, .. }) if error == "authorization_pending" => {
                sleep(Duration::from_millis(device_code.interval_ms as u64)).await;
            }
            Err(other) => return Err(other),
        }
    }
    Err(Error::DeviceCodeTimedOut)
}

/// Builds the Microsoft authorize URL and hands it to `authorize`, which is
/// responsible for driving an interactive browser or embedded webview and
/// resolving with the final URL Microsoft redirects back to (containing
/// `code=` or `error=` in its query string). No local HTTP listener is
/// required: Microsoft redirects to its own "you can close this window"
/// page, so the caller only needs to watch navigation for that URL and read
/// its query string.
///
/// If `config` has no `redirect_uri` set, one is filled in for this call
/// only (`environment.native_client_url()`, e.g.
/// `https://login.live.com/oauth20_desktop.srf`). Leaving `redirect_uri`
/// unset entirely does *not* reliably fall back to that page for every
/// client id — for at least the Java title client id, Microsoft instead
/// attempts a native/broker-style redirect (a non-`http(s)` URL scheme) that
/// no webview can follow, so an explicit `redirect_uri` is required for the
/// embedded-webview flow to work at all.
pub async fn login_with_webview<F, Fut>(
    client: &Client,
    config: &MsaApplicationConfig,
    authorize: F,
) -> Result<MsaToken>
where
    F: FnOnce(Url) -> Fut,
    Fut: Future<Output = Result<Url>>,
{
    let mut config = config.clone();
    if config.redirect_uri.is_none() {
        config.redirect_uri = Some(config.environment.native_client_url());
    }

    let redirect_url = authorize(config.auth_code_url()).await?;
    let code = extract_code(&redirect_url)?;
    request::exchange_auth_code(client, &config, &code).await
}

fn extract_code(redirect_url: &Url) -> Result<String> {
    let mut code = None;
    let mut error = None;
    let mut error_description = None;
    for (key, value) in redirect_url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            "error_description" => error_description = Some(value.into_owned()),
            _ => {}
        }
    }

    if let (Some(error), Some(error_description)) = (error, error_description) {
        return Err(Error::Msa {
            status: reqwest::StatusCode::OK,
            error,
            error_description,
        });
    }
    code.ok_or_else(|| Error::Webview("redirect url did not contain an authorization code".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_code_from_redirect() {
        let url =
            Url::parse("https://login.live.com/oauth20_desktop.srf?code=abc123&lc=1033").unwrap();
        assert_eq!(extract_code(&url).unwrap(), "abc123");
    }

    #[test]
    fn extracts_error_from_redirect() {
        let url = Url::parse("https://login.live.com/oauth20_desktop.srf?error=access_denied&error_description=denied").unwrap();
        let err = extract_code(&url).unwrap_err();
        assert!(matches!(err, Error::Msa { error, .. } if error == "access_denied"));
    }

    #[test]
    fn missing_code_is_a_webview_error() {
        let url = Url::parse("https://login.live.com/oauth20_desktop.srf").unwrap();
        assert!(matches!(extract_code(&url), Err(Error::Webview(_))));
    }

    #[tokio::test]
    async fn webview_login_fills_in_native_client_redirect_when_unset() {
        let client = Client::new();
        let config = MsaApplicationConfig::new(crate::msa::constants::JAVA_TITLE_ID, "scope");
        let captured_url = std::cell::RefCell::new(None);

        // The closure short-circuits before any network call, since we only
        // need to inspect the authorize URL `login_with_webview` builds.
        let result = login_with_webview(&client, &config, |url| {
            *captured_url.borrow_mut() = Some(url);
            async { Err(Error::Webview("stop before network".into())) }
        })
        .await;

        assert!(result.is_err());
        let url = captured_url.into_inner().expect("authorize was called");
        let redirect_uri = url
            .query_pairs()
            .find(|(key, _)| key == "redirect_uri")
            .map(|(_, value)| value.into_owned());
        assert_eq!(
            redirect_uri.as_deref(),
            Some("https://login.live.com/oauth20_desktop.srf")
        );
    }
}
