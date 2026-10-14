use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

pub const FASTESTVPN_API_BASE: &str = "https://api.fastestvpn.com";
pub const FASTESTVPN_APP_BUILD: &str = "5.0.0.11";
pub const FASTESTVPN_PLATFORM_BUILD: &str = "Linux_x86_64";

#[derive(Error, Debug, Clone)]
pub enum ApiError {
    #[error("Authentication failed: {0}")]
    AuthFailed(String),
    #[error("Account inactive or expired: {0}")]
    AccountExpired(String),
    #[error("Network connection error: {0}")]
    Network(String),
    #[error("API parse error: {0}")]
    Parse(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthRequest {
    /// The API identifies accounts by email ("The email field is required." otherwise).
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserData {
    pub username: Option<String>,
    pub email: Option<String>,
    pub status: Option<String>,
    pub expiry_date: Option<String>,
    pub plan_name: Option<String>,
    pub wireguard_private_key: Option<String>,
    pub wireguard_ip: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    /// `true` on success; `false` on validation errors and `null` on bad credentials,
    /// both with HTTP 200. Kept untyped so an unexpected shape doesn't fail parsing.
    pub status: Option<serde_json::Value>,
    pub message: Option<String>,
    pub token: Option<String>,
    pub user: Option<UserData>,
    pub data: Option<serde_json::Value>,
}

const TOKEN_KEYS: [&str; 3] = ["token", "access_token", "api_token"];

impl AuthResponse {
    /// Session token, either top-level or nested in `data`.
    pub fn session_token(&self) -> Option<String> {
        self.token.clone().or_else(|| {
            let data = self.data.as_ref()?;
            TOKEN_KEYS
                .iter()
                .find_map(|k| data.get(*k).and_then(|v| v.as_str()))
                .map(str::to_string)
        })
    }

    /// Account details, from `user`, `data.user`, or `data` itself.
    pub fn user_data(&self) -> UserData {
        if let Some(user) = &self.user {
            return user.clone();
        }
        let Some(data) = &self.data else {
            return UserData::default();
        };
        let source = data.get("user").unwrap_or(data);
        serde_json::from_value(source.clone()).unwrap_or_default()
    }

    /// Top-level keys of `data`, to help map the success payload without logging values.
    pub fn data_keys(&self) -> Vec<String> {
        self.data
            .as_ref()
            .and_then(|d| d.as_object())
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default()
    }

    fn is_success(&self) -> bool {
        match &self.status {
            Some(serde_json::Value::Bool(b)) => *b,
            // Without an explicit status, only trust a response that carries a token.
            _ => self.session_token().is_some(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiServerItem {
    pub id: Option<String>,
    pub name: String,
    pub country: String,
    pub country_code: String,
    pub city: String,
    pub host: String,
    pub ip: Option<String>,
    pub wg_pubkey: Option<String>,
}

pub struct FastestVpnApiClient {
    device_token: String,
    agent: ureq::Agent,
}

impl Default for FastestVpnApiClient {
    fn default() -> Self {
        Self::new()
    }
}

impl FastestVpnApiClient {
    pub fn new() -> Self {
        let device_token = format!(
            "fastest-linux-{}",
            chrono::Utc::now().timestamp_millis()
        );
        // Report 4xx/5xx as responses (not transport errors) so auth failures can be
        // distinguished from network failures, and never hang the worker thread.
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(15)))
            .build()
            .into();
        Self { device_token, agent }
    }

    /// Authenticates with FastestVPN API using username and password.
    pub fn authenticate(&self, username: &str, password: &str) -> Result<AuthResponse, ApiError> {
        let trimmed_user = username.trim();
        let trimmed_pass = password.trim();

        if trimmed_user.is_empty() || trimmed_pass.is_empty() {
            return Err(ApiError::AuthFailed("Username and password cannot be empty".to_string()));
        }

        let url = format!("{}/v2/authenticate", FASTESTVPN_API_BASE);
        let payload = AuthRequest {
            email: trimmed_user.to_string(),
            password: trimmed_pass.to_string(),
        };

        let response = self
            .agent
            .post(&url)
            .header("Content-Type", "application/json; charset=UTF-8")
            .header("x-platform", "linux")
            .header("X-PLATFORM-TOKEN", &self.device_token)
            .header("X-Application-Build-Number", FASTESTVPN_APP_BUILD)
            .header("X-PLATFORM-Build-Number", FASTESTVPN_PLATFORM_BUILD)
            .send_json(&payload)
            .map_err(|e| ApiError::Network(format!("Failed to reach FastestVPN API: {e}")))?;

        let status_code = response.status();
        let body_str = response
            .into_body()
            .read_to_string()
            .map_err(|e| ApiError::Parse(format!("Failed to read response body: {e}")))?;

        if status_code.as_u16() == 401 || status_code.as_u16() == 403 {
            return Err(ApiError::AuthFailed("Invalid username or password".to_string()));
        }
        if !status_code.is_success() {
            return Err(ApiError::Network(format!("FastestVPN API returned HTTP {status_code}")));
        }

        parse_auth_response(&body_str)
    }

    /// Fetches server list from FastestVPN endpoints or returns curated presets.
    pub fn fetch_servers(&self, token: Option<&str>) -> Result<Vec<ApiServerItem>, ApiError> {
        let url = format!("{}/v2/servers", FASTESTVPN_API_BASE);

        let mut req = self
            .agent
            .get(&url)
            .header("x-platform", "linux")
            .header("X-PLATFORM-TOKEN", &self.device_token);

        if let Some(t) = token {
            req = req.header("Authorization", format!("Bearer {t}"));
        }

        // If the live endpoint succeeds, parse it
        if let Some(res) = req.call().ok().filter(|r| r.status().is_success()) {
            if let Ok(body) = res.into_body().read_to_string() {
                if let Ok(servers) = serde_json::from_str::<Vec<ApiServerItem>>(&body) {
                    if !servers.is_empty() {
                        return Ok(servers);
                    }
                }
            }
        }

        // Fallback to built-in FastestVPN servers list
        Ok(get_default_api_servers())
    }
}

/// Parses an `/v2/authenticate` body, mapping rejected logins to `AuthFailed`.
pub fn parse_auth_response(body: &str) -> Result<AuthResponse, ApiError> {
    // The body also carries the full server list (~1 MB), so never echo it whole.
    let parsed: AuthResponse = serde_json::from_str(body).map_err(|e| {
        let snippet: String = body.chars().take(200).collect();
        ApiError::Parse(format!("Failed to parse response: {e}. Body starts: {snippet}"))
    })?;

    if !parsed.is_success() {
        let msg = parsed
            .message
            .clone()
            .unwrap_or_else(|| "Authentication rejected by server".to_string());
        return Err(ApiError::AuthFailed(msg));
    }

    Ok(parsed)
}

/// Fallback server directory with live FastestVPN hostnames and locations.
pub fn get_default_api_servers() -> Vec<ApiServerItem> {
    vec![
        ApiServerItem {
            id: Some("de_01".to_string()),
            name: "Germany - Frankfurt".to_string(),
            country: "Germany".to_string(),
            country_code: "DE".to_string(),
            city: "Frankfurt".to_string(),
            host: "de-01.jumptoserver.com".to_string(),
            ip: None,
            wg_pubkey: None,
        },
        ApiServerItem {
            id: Some("us_ny".to_string()),
            name: "United States - New York".to_string(),
            country: "United States".to_string(),
            country_code: "US".to_string(),
            city: "New York".to_string(),
            host: "us-ny.jumptoserver.com".to_string(),
            ip: None,
            wg_pubkey: None,
        },
        ApiServerItem {
            id: Some("us_dl".to_string()),
            name: "United States - Dallas".to_string(),
            country: "United States".to_string(),
            country_code: "US".to_string(),
            city: "Dallas".to_string(),
            host: "us-dl.jumptoserver.com".to_string(),
            ip: None,
            wg_pubkey: None,
        },
        ApiServerItem {
            id: Some("uk_01".to_string()),
            name: "United Kingdom - London".to_string(),
            country: "United Kingdom".to_string(),
            country_code: "GB".to_string(),
            city: "London".to_string(),
            host: "uk-01.jumptoserver.com".to_string(),
            ip: None,
            wg_pubkey: None,
        },
        ApiServerItem {
            id: Some("nl_01".to_string()),
            name: "Netherlands - Amsterdam".to_string(),
            country: "Netherlands".to_string(),
            country_code: "NL".to_string(),
            city: "Amsterdam".to_string(),
            host: "nl-01.jumptoserver.com".to_string(),
            ip: None,
            wg_pubkey: None,
        },
        ApiServerItem {
            id: Some("fr_01".to_string()),
            name: "France - Paris".to_string(),
            country: "France".to_string(),
            country_code: "FR".to_string(),
            city: "Paris".to_string(),
            host: "fr-01.jumptoserver.com".to_string(),
            ip: None,
            wg_pubkey: None,
        },
        ApiServerItem {
            id: Some("ca_01".to_string()),
            name: "Canada - Toronto".to_string(),
            country: "Canada".to_string(),
            country_code: "CA".to_string(),
            city: "Toronto".to_string(),
            host: "ca-01.jumptoserver.com".to_string(),
            ip: None,
            wg_pubkey: None,
        },
        ApiServerItem {
            id: Some("au_01".to_string()),
            name: "Australia - Sydney".to_string(),
            country: "Australia".to_string(),
            country_code: "AU".to_string(),
            city: "Sydney".to_string(),
            host: "au-01.jumptoserver.com".to_string(),
            ip: None,
            wg_pubkey: None,
        },
        ApiServerItem {
            id: Some("sg_01".to_string()),
            name: "Singapore".to_string(),
            country: "Singapore".to_string(),
            country_code: "SG".to_string(),
            city: "Singapore".to_string(),
            host: "sg-01.jumptoserver.com".to_string(),
            ip: None,
            wg_pubkey: None,
        },
        ApiServerItem {
            id: Some("jp_01".to_string()),
            name: "Japan - Tokyo".to_string(),
            country: "Japan".to_string(),
            country_code: "JP".to_string(),
            city: "Tokyo".to_string(),
            host: "jp-01.jumptoserver.com".to_string(),
            ip: None,
            wg_pubkey: None,
        },
        ApiServerItem {
            id: Some("ch_01".to_string()),
            name: "Switzerland - Zurich".to_string(),
            country: "Switzerland".to_string(),
            country_code: "CH".to_string(),
            city: "Zurich".to_string(),
            host: "ch-01.jumptoserver.com".to_string(),
            ip: None,
            wg_pubkey: None,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shapes observed from api.fastestvpn.com/v2/authenticate (server list trimmed).
    #[test]
    fn rejects_missing_email_field() {
        let body = r#"{"servers":[],"status":false,"message":"The email field is required.","data":{}}"#;
        match parse_auth_response(body) {
            Err(ApiError::AuthFailed(msg)) => assert_eq!(msg, "The email field is required."),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn rejects_invalid_credentials_with_null_status() {
        let body = r#"{"servers":[],"status":null,"message":"Invalid Credentials","data":null}"#;
        match parse_auth_response(body) {
            Err(ApiError::AuthFailed(msg)) => assert_eq!(msg, "Invalid Credentials"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn accepts_success_and_reads_nested_fields() {
        let body = r#"{"status":true,"message":"ok","data":{"token":"abc","user":{"email":"a@b.c","status":"active"}}}"#;
        let res = parse_auth_response(body).expect("success");
        assert_eq!(res.session_token().as_deref(), Some("abc"));
        assert_eq!(res.user_data().email.as_deref(), Some("a@b.c"));
    }
}
