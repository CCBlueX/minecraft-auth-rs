use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::expirable::Expirable;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinecraftToken {
    pub expire_time_ms: i64,
    pub token_type: String,
    pub access_token: String,
}

impl MinecraftToken {
    pub fn authorization_header(&self) -> String {
        format!("{} {}", self.token_type, self.access_token)
    }
}

impl Expirable for MinecraftToken {
    fn expire_time_ms(&self) -> i64 {
        self.expire_time_ms
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinecraftProfile {
    pub id: Uuid,
    pub name: String,
}

impl Expirable for MinecraftProfile {
    // The profile never expires on its own; call `refresh` explicitly after
    // e.g. a name change.
    fn expire_time_ms(&self) -> i64 {
        i64::MAX
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinecraftEntitlements {
    pub items: BTreeSet<String>,
}

impl Expirable for MinecraftEntitlements {
    fn expire_time_ms(&self) -> i64 {
        i64::MAX
    }
}

/// Key material for Minecraft's chat-signing feature. Public/private keys
/// are passed through as the DER bytes Mojang returns (X.509 SubjectPublicKeyInfo
/// and PKCS#8 respectively) rather than parsed into a typed RSA key, so this
/// crate does not need an RSA dependency for a feature most callers won't use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinecraftPlayerCertificates {
    pub expire_time_ms: i64,
    pub public_key_der: Vec<u8>,
    pub private_key_der: Vec<u8>,
    pub public_key_signature: Vec<u8>,
    pub legacy_public_key_signature: Option<Vec<u8>>,
}

impl Expirable for MinecraftPlayerCertificates {
    fn expire_time_ms(&self) -> i64 {
        self.expire_time_ms
    }
}
