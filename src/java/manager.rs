use std::future::Future;

use reqwest::Client;
use serde::de::DeserializeOwned;
use url::Url;
use uuid::Uuid;

use crate::clock;
use crate::crypto::EcKeyPair;
use crate::error::{Error, Result};
use crate::expirable::{Expirable, Holder};
use crate::msa::{self, MsaApplicationConfig, MsaDeviceCode, MsaToken};
use crate::xbl::{self, XblDeviceToken, XblSisuTokens};

use super::model::{
    MinecraftEntitlements, MinecraftPlayerCertificates, MinecraftProfile, MinecraftToken,
};
use super::request as java;
use super::session::JavaLaunchSession;

/// Orchestrates the Microsoft → Xbox Live (SISU) → Minecraft Services login
/// chain for Java Edition and caches each stage, refreshing lazily as
/// callers ask for up-to-date tokens.
pub struct JavaAuthManager {
    http_client: Client,
    application_config: MsaApplicationConfig,
    device_type: String,
    device_key_pair: EcKeyPair,
    device_id: Uuid,

    msa_token: Holder<MsaToken>,
    xbl_device_token: Holder<XblDeviceToken>,
    xbl_sisu: Holder<XblSisuTokens>,
    minecraft_token: Holder<MinecraftToken>,
    minecraft_entitlements: Holder<MinecraftEntitlements>,
    minecraft_profile: Holder<MinecraftProfile>,
    minecraft_player_certificates: Holder<MinecraftPlayerCertificates>,
}

impl JavaAuthManager {
    pub fn builder(http_client: Client) -> Builder {
        Builder::new(http_client)
    }

    /// The Microsoft token, refreshing it first if it has expired.
    pub async fn msa_token(&self) -> Result<MsaToken> {
        self.msa_token
            .get_up_to_date(|cached| self.refresh_msa_token(cached))
            .await
    }

    async fn refresh_msa_token(&self, cached: Option<MsaToken>) -> Result<MsaToken> {
        let cached = cached.ok_or(Error::InvalidState(
            "no msa token stored; the user must sign in again",
        ))?;
        let refresh_token = cached.refresh_token.as_deref().ok_or(Error::InvalidState(
            "msa token has no refresh token; the user must sign in again",
        ))?;
        msa::refresh_token(&self.http_client, &self.application_config, refresh_token).await
    }

    async fn device_token(&self) -> Result<XblDeviceToken> {
        self.xbl_device_token
            .get_up_to_date(|_| self.refresh_device_token())
            .await
    }

    async fn refresh_device_token(&self) -> Result<XblDeviceToken> {
        let offset = clock::offset_seconds(&self.http_client).await;
        xbl::device_authenticate(
            &self.http_client,
            &self.device_type,
            self.device_id,
            &self.device_key_pair,
            offset,
        )
        .await
    }

    async fn sisu_tokens(&self) -> Result<XblSisuTokens> {
        self.xbl_sisu
            .get_up_to_date(|_| self.refresh_sisu_tokens())
            .await
    }

    async fn refresh_sisu_tokens(&self) -> Result<XblSisuTokens> {
        let msa_token = self.msa_token().await?;
        let device_token = self.device_token().await?;
        let offset = clock::offset_seconds(&self.http_client).await;
        xbl::sisu_authorize(
            &self.http_client,
            &self.application_config,
            &msa_token,
            &device_token,
            &self.device_key_pair,
            offset,
            xbl::constants::JAVA_XSTS_RELYING_PARTY,
        )
        .await
    }

    /// The Minecraft Services access token, refreshing the whole chain
    /// above it as needed.
    pub async fn minecraft_token(&self) -> Result<MinecraftToken> {
        self.minecraft_token
            .get_up_to_date(|_| self.refresh_minecraft_token())
            .await
    }

    async fn refresh_minecraft_token(&self) -> Result<MinecraftToken> {
        let sisu = self.sisu_tokens().await?;
        java::launcher_login(&self.http_client, &sisu.xsts_token).await
    }

    /// The account's store entitlements (owns-Minecraft / owns-Minecraft-legacy).
    /// Never expires once fetched — call this again explicitly to force a refresh.
    pub async fn entitlements(&self) -> Result<MinecraftEntitlements> {
        self.minecraft_entitlements
            .get_up_to_date(|_| async {
                let token = self.minecraft_token().await?;
                java::entitlements(&self.http_client, &token).await
            })
            .await
    }

    /// The account's Minecraft profile (name, UUID). Never expires once
    /// fetched — call this again explicitly after e.g. a name change.
    pub async fn profile(&self) -> Result<MinecraftProfile> {
        self.minecraft_profile
            .get_up_to_date(|_| async {
                let token = self.minecraft_token().await?;
                java::profile(&self.http_client, &token).await
            })
            .await
    }

    /// Chat-signing key material. Optional — most launchers don't need this.
    pub async fn player_certificates(&self) -> Result<MinecraftPlayerCertificates> {
        self.minecraft_player_certificates
            .get_up_to_date(|_| async {
                let token = self.minecraft_token().await?;
                java::player_certificates(&self.http_client, &token).await
            })
            .await
    }

    /// Player name, UUID and access token, ready to pass as game launch arguments.
    pub async fn launch_session(&self) -> Result<JavaLaunchSession> {
        let token = self.minecraft_token().await?;
        let profile = self.profile().await?;
        Ok(JavaLaunchSession {
            player_name: profile.name,
            player_uuid: profile.id,
            access_token: token.access_token,
        })
    }

    pub fn application_config(&self) -> &MsaApplicationConfig {
        &self.application_config
    }

    pub async fn to_json(&self) -> Result<serde_json::Value> {
        let mut json = serde_json::json!({
            "_save_version": 1,
            "application_config": self.application_config,
            "device_type": self.device_type,
            "device_key_pair": self.device_key_pair,
            "device_id": self.device_id,
            "msa_token": self.msa_token.cached().await,
        });
        insert_if_present(
            &mut json,
            "xbl_device_token",
            self.xbl_device_token.cached().await,
        )?;
        insert_if_present(&mut json, "xbl_sisu", self.xbl_sisu.cached().await)?;
        insert_if_present(
            &mut json,
            "minecraft_token",
            self.minecraft_token.cached().await,
        )?;
        insert_if_present(
            &mut json,
            "minecraft_entitlements",
            self.minecraft_entitlements.cached().await,
        )?;
        insert_if_present(
            &mut json,
            "minecraft_profile",
            self.minecraft_profile.cached().await,
        )?;
        insert_if_present(
            &mut json,
            "minecraft_player_certificates",
            self.minecraft_player_certificates.cached().await,
        )?;
        Ok(json)
    }

    pub fn from_json(http_client: Client, json: &serde_json::Value) -> Result<Self> {
        Ok(Self {
            http_client,
            application_config: field(json, "application_config")?,
            device_type: field(json, "device_type")?,
            device_key_pair: field(json, "device_key_pair")?,
            device_id: field(json, "device_id")?,
            msa_token: Holder::with_value(field(json, "msa_token")?),
            xbl_device_token: optional_holder(json.get("xbl_device_token"))?,
            xbl_sisu: optional_holder(json.get("xbl_sisu"))?,
            minecraft_token: optional_holder(json.get("minecraft_token"))?,
            minecraft_entitlements: optional_holder(json.get("minecraft_entitlements"))?,
            minecraft_profile: optional_holder(json.get("minecraft_profile"))?,
            minecraft_player_certificates: optional_holder(
                json.get("minecraft_player_certificates"),
            )?,
        })
    }
}

fn field<T: DeserializeOwned>(json: &serde_json::Value, key: &str) -> Result<T> {
    Ok(serde_json::from_value(json[key].clone())?)
}

fn insert_if_present<T: serde::Serialize>(
    json: &mut serde_json::Value,
    key: &str,
    value: Option<T>,
) -> Result<()> {
    if let Some(value) = value {
        json[key] = serde_json::to_value(value)?;
    }
    Ok(())
}

fn optional_holder<T>(value: Option<&serde_json::Value>) -> Result<Holder<T>>
where
    T: Expirable + Clone + DeserializeOwned,
{
    match value {
        Some(value) if !value.is_null() => {
            Ok(Holder::with_value(serde_json::from_value(value.clone())?))
        }
        _ => Ok(Holder::empty()),
    }
}

pub struct Builder {
    http_client: Client,
    application_config: MsaApplicationConfig,
    device_type: String,
    device_key_pair: Option<EcKeyPair>,
    device_id: Option<Uuid>,
}

impl Builder {
    fn new(http_client: Client) -> Self {
        Self {
            http_client,
            application_config: MsaApplicationConfig::java_default(),
            device_type: "Win32".to_string(),
            device_key_pair: None,
            device_id: None,
        }
    }

    pub fn application_config(mut self, config: MsaApplicationConfig) -> Self {
        self.application_config = config;
        self
    }

    pub fn device_type(mut self, device_type: impl Into<String>) -> Self {
        self.device_type = device_type.into();
        self
    }

    pub fn device_key_pair(mut self, device_key_pair: EcKeyPair) -> Self {
        self.device_key_pair = Some(device_key_pair);
        self
    }

    pub fn device_id(mut self, device_id: Uuid) -> Self {
        self.device_id = Some(device_id);
        self
    }

    /// Finalizes the manager with an already-obtained MSA token — useful
    /// when migrating a session from elsewhere.
    pub fn login_msa_token(self, msa_token: MsaToken) -> JavaAuthManager {
        JavaAuthManager {
            http_client: self.http_client,
            application_config: self.application_config,
            device_type: self.device_type,
            device_key_pair: self.device_key_pair.unwrap_or_else(EcKeyPair::generate),
            device_id: self.device_id.unwrap_or_else(Uuid::new_v4),
            msa_token: Holder::with_value(msa_token),
            xbl_device_token: Holder::empty(),
            xbl_sisu: Holder::empty(),
            minecraft_token: Holder::empty(),
            minecraft_entitlements: Holder::empty(),
            minecraft_profile: Holder::empty(),
            minecraft_player_certificates: Holder::empty(),
        }
    }

    pub async fn login_refresh_token(self, refresh_token: &str) -> Result<JavaAuthManager> {
        let msa_token =
            msa::refresh_token(&self.http_client, &self.application_config, refresh_token).await?;
        Ok(self.login_msa_token(msa_token))
    }

    /// Signs in via the device code flow: `on_code` is called once the code
    /// has been requested, to show the user where and what to enter.
    pub async fn login_device_code<F>(self, on_code: F) -> Result<JavaAuthManager>
    where
        F: FnOnce(&MsaDeviceCode),
    {
        let msa_token =
            msa::login_with_device_code(&self.http_client, &self.application_config, on_code)
                .await?;
        Ok(self.login_msa_token(msa_token))
    }

    /// Signs in via an interactive browser/webview: `authorize` receives the
    /// Microsoft authorize URL to navigate to, and must resolve with the
    /// final redirect URL once Microsoft sends the user back.
    pub async fn login_webview<F, Fut>(self, authorize: F) -> Result<JavaAuthManager>
    where
        F: FnOnce(Url) -> Fut,
        Fut: Future<Output = Result<Url>>,
    {
        let msa_token =
            msa::login_with_webview(&self.http_client, &self.application_config, authorize).await?;
        Ok(self.login_msa_token(msa_token))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Regression: `refresh_msa_token` used to re-read the cached token via
    /// `Holder::cached()` while `get_up_to_date` was already holding that
    /// holder's lock. `tokio::sync::Mutex` is not reentrant, so every
    /// expired-token refresh parked forever — a launcher calling this sat on
    /// "refreshing session" with no error and nothing in its log.
    #[tokio::test]
    async fn expired_msa_token_refresh_does_not_deadlock() {
        let manager = JavaAuthManager::builder(Client::new()).login_msa_token(MsaToken {
            expire_time_ms: 0,
            access_token: "expired".to_string(),
            refresh_token: None,
        });

        // No refresh token means this fails before any network call — so the
        // only thing that can make it exceed the timeout is the deadlock.
        let result = tokio::time::timeout(Duration::from_secs(5), manager.msa_token())
            .await
            .expect("msa_token() must return rather than hang on an expired token");

        assert!(matches!(result, Err(Error::InvalidState(_))));
    }

    #[tokio::test]
    async fn valid_msa_token_is_returned_from_cache() {
        let manager = JavaAuthManager::builder(Client::new()).login_msa_token(MsaToken {
            expire_time_ms: crate::expirable::now_ms() + 60_000,
            access_token: "still-good".to_string(),
            refresh_token: Some("refresh".to_string()),
        });

        let token = tokio::time::timeout(Duration::from_secs(5), manager.msa_token())
            .await
            .expect("cached lookup must not hang")
            .unwrap();

        assert_eq!(token.access_token, "still-good");
    }
}
