use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::json;
use uuid::Uuid;

use crate::crypto::EcKeyPair;
use crate::error::{Error, Result};
use crate::msa::{MsaApplicationConfig, MsaToken};

use super::constants;
use super::error::xbl_error;
use super::model::{XblDeviceToken, XblSisuTokens, XblTitleToken, XblUserToken, XblXstsToken};
use super::sign::{signature_header, SignaturePayload};

const DEVICE_AUTHENTICATE_URL: &str = "https://device.auth.xboxlive.com/device/authenticate";
const SISU_AUTHORIZE_URL: &str = "https://sisu.xboxlive.com/authorize";

#[derive(Deserialize)]
struct XblTokenResponse {
    #[serde(rename = "NotAfter")]
    not_after: String,
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: DisplayClaims,
}

#[derive(Deserialize, Default)]
struct DisplayClaims {
    xdi: Option<DeviceClaim>,
    xti: Option<TitleClaim>,
    xui: Option<Vec<UserClaim>>,
}

#[derive(Deserialize)]
struct DeviceClaim {
    did: String,
}

#[derive(Deserialize)]
struct TitleClaim {
    tid: String,
}

#[derive(Deserialize)]
struct UserClaim {
    uhs: String,
}

impl XblTokenResponse {
    fn into_device_token(self) -> Result<XblDeviceToken> {
        let device_id = self
            .display_claims
            .xdi
            .ok_or_else(|| missing_claim("xdi.did"))?
            .did;
        Ok(XblDeviceToken {
            expire_time_ms: parse_instant_ms(&self.not_after)?,
            token: self.token,
            device_id,
        })
    }

    fn into_user_token(self) -> Result<XblUserToken> {
        let user_hash = first_user_hash(self.display_claims.xui)?;
        Ok(XblUserToken {
            expire_time_ms: parse_instant_ms(&self.not_after)?,
            token: self.token,
            user_hash,
        })
    }

    fn into_title_token(self) -> Result<XblTitleToken> {
        let title_id = self
            .display_claims
            .xti
            .ok_or_else(|| missing_claim("xti.tid"))?
            .tid;
        Ok(XblTitleToken {
            expire_time_ms: parse_instant_ms(&self.not_after)?,
            token: self.token,
            title_id,
        })
    }

    fn into_xsts_token(self) -> Result<XblXstsToken> {
        let user_hash = first_user_hash(self.display_claims.xui)?;
        Ok(XblXstsToken {
            expire_time_ms: parse_instant_ms(&self.not_after)?,
            token: self.token,
            user_hash,
        })
    }
}

fn first_user_hash(xui: Option<Vec<UserClaim>>) -> Result<String> {
    xui.and_then(|claims| claims.into_iter().next())
        .map(|claim| claim.uhs)
        .ok_or_else(|| missing_claim("xui[0].uhs"))
}

fn missing_claim(what: &str) -> Error {
    Error::UnexpectedResponse {
        endpoint: "xbox live",
        reason: format!("missing {what} claim"),
    }
}

fn parse_instant_ms(s: &str) -> Result<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.timestamp_millis())
        .map_err(|e| Error::UnexpectedResponse {
            endpoint: "xbox live",
            reason: format!("invalid timestamp {s:?}: {e}"),
        })
}

fn x_err_header(response: &reqwest::Response) -> Option<u64> {
    response.headers().get("X-Err")?.to_str().ok()?.parse().ok()
}

async fn xbl_send<T: DeserializeOwned>(response: reqwest::Response) -> Result<T> {
    let status = response.status();
    if let Some(code) = x_err_header(&response) {
        return Err(xbl_error(status, code));
    }
    let bytes = response.bytes().await?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    if let Some(code) = value.get("XErr").and_then(|v| v.as_u64()) {
        return Err(xbl_error(status, code));
    }
    if !status.is_success() {
        return Err(Error::UnexpectedResponse {
            endpoint: "xbox live",
            reason: format!("status {status}"),
        });
    }
    Ok(serde_json::from_value(value)?)
}

pub async fn device_authenticate(
    client: &Client,
    device_type: &str,
    device_id: Uuid,
    key: &EcKeyPair,
    clock_offset_seconds: i64,
) -> Result<XblDeviceToken> {
    let body = json!({
        "Properties": {
            "DeviceType": device_type,
            "Id": format!("{{{device_id}}}"),
            "AuthMethod": "ProofOfPossession",
            "ProofKey": key.proof_key_jwk(),
        },
        "RelyingParty": constants::XBL_AUTH_RELYING_PARTY,
        "TokenType": "JWT",
    });
    let bytes = serde_json::to_vec(&body)?;
    let signature = signature_header(
        key,
        clock_offset_seconds,
        &SignaturePayload {
            method: "POST",
            path_and_query: "/device/authenticate",
            authorization_header: None,
            body: &bytes,
        },
    );

    let response = client
        .post(DEVICE_AUTHENTICATE_URL)
        .header("Content-Type", "application/json")
        .header("x-xbl-contract-version", "1")
        .header("Signature", signature)
        .body(bytes)
        .timeout(crate::REQUEST_TIMEOUT)
        .send()
        .await?;

    xbl_send::<XblTokenResponse>(response)
        .await?
        .into_device_token()
}

#[derive(Deserialize)]
struct SisuResponse {
    #[serde(rename = "UserToken")]
    user_token: XblTokenResponse,
    #[serde(rename = "TitleToken")]
    title_token: XblTokenResponse,
    #[serde(rename = "AuthorizationToken")]
    authorization_token: XblTokenResponse,
}

/// One signed call that returns user, title and XSTS tokens together for
/// `relying_party`. Requires a title client id (see
/// [`MsaApplicationConfig::is_title_client_id`]).
pub async fn sisu_authorize(
    client: &Client,
    application_config: &MsaApplicationConfig,
    msa_token: &MsaToken,
    device_token: &XblDeviceToken,
    key: &EcKeyPair,
    clock_offset_seconds: i64,
    relying_party: &str,
) -> Result<XblSisuTokens> {
    if !application_config.is_title_client_id() {
        return Err(Error::InvalidState(
            "XBL SISU authorization requires a title client id",
        ));
    }

    let body = json!({
        "Sandbox": "RETAIL",
        "UseModernGamertag": true,
        "AppId": application_config.client_id,
        "AccessToken": format!("t={}", msa_token.access_token),
        "DeviceToken": device_token.token,
        "ProofKey": key.proof_key_jwk(),
        "RelyingParty": relying_party,
    });
    let bytes = serde_json::to_vec(&body)?;
    let signature = signature_header(
        key,
        clock_offset_seconds,
        &SignaturePayload {
            method: "POST",
            path_and_query: "/authorize",
            authorization_header: None,
            body: &bytes,
        },
    );

    let response = client
        .post(SISU_AUTHORIZE_URL)
        .header("Content-Type", "application/json")
        .header("Signature", signature)
        .body(bytes)
        .timeout(crate::REQUEST_TIMEOUT)
        .send()
        .await?;

    let raw: SisuResponse = xbl_send(response).await?;
    Ok(XblSisuTokens {
        user_token: raw.user_token.into_user_token()?,
        title_token: raw.title_token.into_title_token()?,
        xsts_token: raw.authorization_token.into_xsts_token()?,
    })
}
