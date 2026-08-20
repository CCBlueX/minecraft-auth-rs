use std::collections::HashMap;

use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize};

use crate::error::{Error, Result};
use crate::expirable::now_ms;

use super::config::{MsaApplicationConfig, MsaEnvironment};
use super::model::{MsaDeviceCode, MsaToken};

#[derive(Deserialize)]
struct DeviceCodeResponse {
    expires_in: i64,
    interval: i64,
    device_code: String,
    user_code: String,
    verification_uri: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    expires_in: i64,
    access_token: String,
    refresh_token: Option<String>,
}

impl From<TokenResponse> for MsaToken {
    fn from(response: TokenResponse) -> Self {
        MsaToken {
            expire_time_ms: now_ms() + response.expires_in * 1000,
            access_token: response.access_token,
            refresh_token: response.refresh_token,
        }
    }
}

async fn post_form<T: DeserializeOwned>(
    client: &Client,
    url: &str,
    form: &HashMap<&str, &str>,
) -> Result<T> {
    let response = client.post(url).form(form).send().await?;
    let status = response.status();
    let json: serde_json::Value = response.json().await?;

    if let (Some(error), Some(error_description)) = (
        json.get("error").and_then(|v| v.as_str()),
        json.get("error_description").and_then(|v| v.as_str()),
    ) {
        return Err(Error::Msa {
            status,
            error: error.to_string(),
            error_description: error_description.to_string(),
        });
    }

    Ok(serde_json::from_value(json)?)
}

pub async fn request_device_code(
    client: &Client,
    config: &MsaApplicationConfig,
) -> Result<MsaDeviceCode> {
    let mut form = HashMap::from([
        ("client_id", config.client_id.as_str()),
        ("scope", config.scope.as_str()),
    ]);
    if config.environment == MsaEnvironment::Live {
        form.insert("response_type", "device_code");
    }

    let response: DeviceCodeResponse =
        post_form(client, &config.environment.device_code_url(), &form).await?;
    Ok(MsaDeviceCode {
        expire_time_ms: now_ms() + response.expires_in * 1000,
        interval_ms: response.interval * 1000,
        device_code: response.device_code,
        user_code: response.user_code,
        verification_uri: response.verification_uri,
    })
}

pub async fn poll_device_code_token(
    client: &Client,
    config: &MsaApplicationConfig,
    device_code: &str,
) -> Result<MsaToken> {
    let form = HashMap::from([
        ("client_id", config.client_id.as_str()),
        ("grant_type", "device_code"),
        ("device_code", device_code),
    ]);
    let response: TokenResponse = post_form(client, &config.environment.token_url(), &form).await?;
    Ok(response.into())
}

pub async fn refresh_token(
    client: &Client,
    config: &MsaApplicationConfig,
    refresh_token: &str,
) -> Result<MsaToken> {
    let mut form = HashMap::from([
        ("client_id", config.client_id.as_str()),
        ("scope", config.scope.as_str()),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
    ]);
    if let Some(secret) = &config.client_secret {
        form.insert("client_secret", secret.as_str());
    }

    let response: TokenResponse = post_form(client, &config.environment.token_url(), &form).await?;
    Ok(response.into())
}

pub async fn exchange_auth_code(
    client: &Client,
    config: &MsaApplicationConfig,
    auth_code: &str,
) -> Result<MsaToken> {
    let mut form = HashMap::from([
        ("client_id", config.client_id.as_str()),
        ("scope", config.scope.as_str()),
        ("grant_type", "authorization_code"),
        ("code", auth_code),
    ]);
    if let Some(secret) = &config.client_secret {
        form.insert("client_secret", secret.as_str());
    }
    if let Some(redirect_uri) = &config.redirect_uri {
        form.insert("redirect_uri", redirect_uri.as_str());
    }

    let response: TokenResponse = post_form(client, &config.environment.token_url(), &form).await?;
    Ok(response.into())
}
