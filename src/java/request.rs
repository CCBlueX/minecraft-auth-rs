use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::{Client, StatusCode};
use serde::{de::DeserializeOwned, Deserialize};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::expirable::now_ms;
use crate::xbl::XblXstsToken;

use super::model::{
    MinecraftEntitlements, MinecraftPlayerCertificates, MinecraftProfile, MinecraftToken,
};

const LAUNCHER_LOGIN_URL: &str = "https://api.minecraftservices.com/launcher/login";
const ENTITLEMENTS_URL: &str = "https://api.minecraftservices.com/entitlements/mcstore";
const PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";
const PLAYER_CERTIFICATES_URL: &str = "https://api.minecraftservices.com/player/certificates";

async fn handle<T: DeserializeOwned>(response: reqwest::Response) -> Result<T> {
    let status = response.status();
    let bytes = response.bytes().await?;
    if !status.is_success() {
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
        let error = value.get("error").and_then(|v| v.as_str());
        let error_message = value.get("errorMessage").and_then(|v| v.as_str());
        return Err(match (error, error_message) {
            (Some(error), Some(message)) => Error::MinecraftServices {
                status,
                error: error.to_string(),
                error_message: message.to_string(),
            },
            (None, Some(message)) => Error::UnexpectedResponse {
                endpoint: "minecraft services",
                reason: message.to_string(),
            },
            _ => Error::UnexpectedResponse {
                endpoint: "minecraft services",
                reason: format!("status {status}"),
            },
        });
    }
    Ok(serde_json::from_slice(&bytes)?)
}

#[derive(Deserialize)]
struct LauncherLoginResponse {
    expires_in: i64,
    token_type: String,
    access_token: String,
}

pub async fn launcher_login(client: &Client, xsts_token: &XblXstsToken) -> Result<MinecraftToken> {
    let body = serde_json::json!({
        "platform": "PC_LAUNCHER",
        "xtoken": xsts_token.authorization_header(),
    });
    let response = client.post(LAUNCHER_LOGIN_URL).json(&body).send().await?;
    let raw: LauncherLoginResponse = handle(response).await?;
    Ok(MinecraftToken {
        expire_time_ms: now_ms() + raw.expires_in * 1000,
        token_type: raw.token_type,
        access_token: raw.access_token,
    })
}

#[derive(Deserialize)]
struct EntitlementItem {
    name: String,
}

#[derive(Deserialize)]
struct EntitlementsResponse {
    items: Vec<EntitlementItem>,
}

pub async fn entitlements(
    client: &Client,
    token: &MinecraftToken,
) -> Result<MinecraftEntitlements> {
    let response = client
        .get(ENTITLEMENTS_URL)
        .header(reqwest::header::AUTHORIZATION, token.authorization_header())
        .send()
        .await?;
    let raw: EntitlementsResponse = handle(response).await?;
    Ok(MinecraftEntitlements {
        items: raw.items.into_iter().map(|item| item.name).collect(),
    })
}

#[derive(Deserialize)]
struct ProfileResponse {
    id: String,
    name: String,
}

pub async fn profile(client: &Client, token: &MinecraftToken) -> Result<MinecraftProfile> {
    let response = client
        .get(PROFILE_URL)
        .header(reqwest::header::AUTHORIZATION, token.authorization_header())
        .send()
        .await?;
    if response.status() == StatusCode::NOT_FOUND {
        return Err(Error::ProfileNotFound);
    }
    let raw: ProfileResponse = handle(response).await?;
    let id = Uuid::parse_str(&raw.id).map_err(|e| Error::UnexpectedResponse {
        endpoint: "minecraft profile",
        reason: format!("invalid uuid {:?}: {e}", raw.id),
    })?;
    Ok(MinecraftProfile { id, name: raw.name })
}

#[derive(Deserialize)]
struct PlayerCertificatesResponse {
    #[serde(rename = "expiresAt")]
    expires_at: String,
    #[serde(rename = "keyPair")]
    key_pair: KeyPairResponse,
    #[serde(rename = "publicKeySignatureV2")]
    public_key_signature_v2: String,
    #[serde(rename = "publicKeySignature")]
    public_key_signature: Option<String>,
}

#[derive(Deserialize)]
struct KeyPairResponse {
    #[serde(rename = "publicKey")]
    public_key: String,
    #[serde(rename = "privateKey")]
    private_key: String,
}

pub async fn player_certificates(
    client: &Client,
    token: &MinecraftToken,
) -> Result<MinecraftPlayerCertificates> {
    let response = client
        .post(PLAYER_CERTIFICATES_URL)
        .header(reqwest::header::AUTHORIZATION, token.authorization_header())
        .send()
        .await?;
    let raw: PlayerCertificatesResponse = handle(response).await?;

    let expire_time_ms = chrono::DateTime::parse_from_rfc3339(&raw.expires_at)
        .map(|dt| dt.timestamp_millis())
        .map_err(|e| Error::UnexpectedResponse {
            endpoint: "player certificates",
            reason: format!("invalid timestamp {:?}: {e}", raw.expires_at),
        })?;

    Ok(MinecraftPlayerCertificates {
        expire_time_ms,
        public_key_der: decode_pem_body(&raw.key_pair.public_key)?,
        private_key_der: decode_pem_body(&raw.key_pair.private_key)?,
        public_key_signature: decode_base64(&raw.public_key_signature_v2)?,
        legacy_public_key_signature: raw
            .public_key_signature
            .as_deref()
            .map(decode_base64)
            .transpose()?,
    })
}

fn decode_base64(s: &str) -> Result<Vec<u8>> {
    STANDARD.decode(s).map_err(|e| Error::UnexpectedResponse {
        endpoint: "player certificates",
        reason: e.to_string(),
    })
}

fn decode_pem_body(s: &str) -> Result<Vec<u8>> {
    let stripped: String = s
        .replace("-----BEGIN RSA PUBLIC KEY-----", "")
        .replace("-----END RSA PUBLIC KEY-----", "")
        .replace("-----BEGIN RSA PRIVATE KEY-----", "")
        .replace("-----END RSA PRIVATE KEY-----", "")
        .split_whitespace()
        .collect();
    decode_base64(&stripped)
}
