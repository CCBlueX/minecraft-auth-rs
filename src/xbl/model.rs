use serde::{Deserialize, Serialize};

use crate::expirable::Expirable;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XblDeviceToken {
    pub expire_time_ms: i64,
    pub token: String,
    pub device_id: String,
}

impl Expirable for XblDeviceToken {
    fn expire_time_ms(&self) -> i64 {
        self.expire_time_ms
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XblUserToken {
    pub expire_time_ms: i64,
    pub token: String,
    pub user_hash: String,
}

impl Expirable for XblUserToken {
    fn expire_time_ms(&self) -> i64 {
        self.expire_time_ms
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XblTitleToken {
    pub expire_time_ms: i64,
    pub token: String,
    pub title_id: String,
}

impl Expirable for XblTitleToken {
    fn expire_time_ms(&self) -> i64 {
        self.expire_time_ms
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XblXstsToken {
    pub expire_time_ms: i64,
    pub token: String,
    pub user_hash: String,
}

impl XblXstsToken {
    pub fn authorization_header(&self) -> String {
        format!("XBL3.0 x={};{}", self.user_hash, self.token)
    }
}

impl Expirable for XblXstsToken {
    fn expire_time_ms(&self) -> i64 {
        self.expire_time_ms
    }
}

/// The combined result of one SISU `/authorize` call: user, title and XSTS
/// tokens for a single relying party, obtained atomically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XblSisuTokens {
    pub user_token: XblUserToken,
    pub title_token: XblTitleToken,
    pub xsts_token: XblXstsToken,
}

impl Expirable for XblSisuTokens {
    /// All three tokens are always refreshed together, so the XSTS token's
    /// expiry (the only one consumed downstream) gates the whole group.
    fn expire_time_ms(&self) -> i64 {
        self.xsts_token.expire_time_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xsts_authorization_header_matches_wire_format() {
        let token = XblXstsToken {
            expire_time_ms: 0,
            token: "the-token".to_string(),
            user_hash: "the-hash".to_string(),
        };
        assert_eq!(token.authorization_header(), "XBL3.0 x=the-hash;the-token");
    }
}
