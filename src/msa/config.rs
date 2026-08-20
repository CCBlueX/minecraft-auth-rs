use serde::{Deserialize, Serialize};

pub mod constants {
    pub const JAVA_TITLE_ID: &str = "00000000402b5328";
    pub const SCOPE_TITLE_AUTH: &str = "service::user.auth.xboxlive.com::MBI_SSL";
    pub const SCOPE_NO_OFFLINE_ACCESS: &str = "XboxLive.signin";
    pub const SCOPE_OFFLINE_ACCESS: &str = "XboxLive.signin XboxLive.offline_access";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MsaEnvironment {
    #[default]
    Live,
    MicrosoftOnlineCommon,
    MicrosoftOnlineConsumers,
}

impl MsaEnvironment {
    fn base_url(self) -> &'static str {
        match self {
            Self::Live => "https://login.live.com/",
            Self::MicrosoftOnlineCommon => "https://login.microsoftonline.com/common/oauth2/",
            Self::MicrosoftOnlineConsumers => "https://login.microsoftonline.com/consumers/oauth2/",
        }
    }

    pub fn device_code_url(self) -> String {
        let path = match self {
            Self::Live => "oauth20_connect.srf",
            _ => "v2.0/devicecode",
        };
        format!("{}{path}", self.base_url())
    }

    pub fn token_url(self) -> String {
        let path = match self {
            Self::Live => "oauth20_token.srf",
            _ => "v2.0/token",
        };
        format!("{}{path}", self.base_url())
    }

    pub fn authorize_url(self) -> String {
        let path = match self {
            Self::Live => "oauth20_authorize.srf",
            _ => "v2.0/authorize",
        };
        format!("{}{path}", self.base_url())
    }

    /// Where Microsoft redirects installed apps that register no
    /// `redirect_uri` of their own. The webview login flow watches
    /// navigation for this prefix to recognize the completed sign-in.
    pub fn native_client_url(self) -> String {
        let path = match self {
            Self::Live => "oauth20_desktop.srf",
            _ => "nativeclient",
        };
        format!("{}{path}", self.base_url())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MsaApplicationConfig {
    pub client_id: String,
    pub scope: String,
    pub client_secret: Option<String>,
    pub redirect_uri: Option<String>,
    #[serde(default)]
    pub environment: MsaEnvironment,
}

impl MsaApplicationConfig {
    pub fn new(client_id: impl Into<String>, scope: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            scope: scope.into(),
            client_secret: None,
            redirect_uri: None,
            environment: MsaEnvironment::Live,
        }
    }

    /// The application config used by the official Minecraft Java launcher.
    pub fn java_default() -> Self {
        Self::new(constants::JAVA_TITLE_ID, constants::SCOPE_TITLE_AUTH)
    }

    pub fn with_redirect_uri(mut self, redirect_uri: impl Into<String>) -> Self {
        self.redirect_uri = Some(redirect_uri.into());
        self
    }

    pub fn is_title_client_id(&self) -> bool {
        !is_dashed_uuid(&self.client_id)
    }

    pub fn auth_code_url(&self) -> url::Url {
        let mut url = url::Url::parse(&self.environment.authorize_url()).expect("static url");
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("client_id", &self.client_id);
            query.append_pair("scope", &self.scope);
            if let Some(redirect_uri) = &self.redirect_uri {
                query.append_pair("redirect_uri", redirect_uri);
            }
            query.append_pair("response_type", "code");
            query.append_pair("response_mode", "query");
        }
        url
    }
}

fn is_dashed_uuid(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() == 36
        && bytes[8] == b'-'
        && bytes[13] == b'-'
        && bytes[18] == b'-'
        && bytes[23] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| matches!(i, 8 | 13 | 18 | 23) || b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_title_id_is_not_a_dashed_uuid() {
        let config = MsaApplicationConfig::java_default();
        assert!(config.is_title_client_id());
    }

    #[test]
    fn dashed_client_id_is_not_a_title_id() {
        let config = MsaApplicationConfig::new("00000000-0000-0000-0000-000000000000", "scope");
        assert!(!config.is_title_client_id());
    }

    #[test]
    fn auth_code_url_carries_required_parameters() {
        let config = MsaApplicationConfig::new("client", "scope")
            .with_redirect_uri("http://localhost/callback");
        let url = config.auth_code_url();
        let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(pairs.get("client_id").unwrap(), "client");
        assert_eq!(pairs.get("scope").unwrap(), "scope");
        assert_eq!(
            pairs.get("redirect_uri").unwrap(),
            "http://localhost/callback"
        );
        assert_eq!(pairs.get("response_type").unwrap(), "code");
        assert_eq!(pairs.get("response_mode").unwrap(), "query");
    }
}
