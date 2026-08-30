use std::{
    env, fmt,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    str::FromStr,
    time::Duration,
};

use base64::Engine;
use thiserror::Error;
use url::Url;
use uuid::Uuid;

const DEFAULT_BODY_LIMIT: usize = 8 * 1024 * 1024;
const DEFAULT_BLOB_PROJECT_QUOTA: u64 = 1024 * 1024 * 1024;
const DEFAULT_AUTH_RATE_LIMIT_PER_MINUTE: u32 = 30;
const DEFAULT_RECOVERY_RATE_LIMIT_PER_MINUTE: u32 = 10;
const DEFAULT_SESSION_RATE_LIMIT_PER_MINUTE: u32 = 600;
const INDEPENDENT_CRYPTO_AUDIT_COMPLETE: bool = false;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeploymentEnvironment {
    Development,
    Production,
}

impl FromStr for DeploymentEnvironment {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "development" => Ok(Self::Development),
            "production" => Ok(Self::Production),
            _ => Err(()),
        }
    }
}

#[derive(Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub base_url: Url,
    pub database_url: SecretString,
    pub database_max_connections: u32,
    pub migrations_dir: PathBuf,
    pub blob_dir: PathBuf,
    pub archive_dir: PathBuf,
    pub body_limit_bytes: usize,
    pub blob_max_file_bytes: u64,
    pub blob_project_quota_bytes: u64,
    pub cors_origins: Vec<Url>,
    pub session_ttl: Duration,
    pub agent_work_lease: Duration,
    pub ceremony_ttl: Duration,
    pub email_verification_ttl: Duration,
    pub account_recovery_ttl: Duration,
    pub email_outbox_key: SecretKey,
    pub archive_signing_key: SecretKey,
    pub archive_signing_key_id: Uuid,
    pub metrics_token: Option<SecretString>,
    pub auth_rate_limit_per_minute: u32,
    pub recovery_rate_limit_per_minute: u32,
    pub session_rate_limit_per_minute: u32,
    pub webauthn_rp_id: String,
    pub webauthn_rp_name: String,
    pub deployment_environment: DeploymentEnvironment,
    pub enable_experimental_crypto_for_development: bool,
}

impl fmt::Debug for Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Config")
            .field("bind_addr", &self.bind_addr)
            .field("base_url", &self.base_url)
            .field("database_url", &self.database_url)
            .field("database_max_connections", &self.database_max_connections)
            .field("migrations_dir", &self.migrations_dir)
            .field("blob_dir", &self.blob_dir)
            .field("archive_dir", &self.archive_dir)
            .field("body_limit_bytes", &self.body_limit_bytes)
            .field("blob_max_file_bytes", &self.blob_max_file_bytes)
            .field("blob_project_quota_bytes", &self.blob_project_quota_bytes)
            .field("cors_origins", &self.cors_origins)
            .field("session_ttl", &self.session_ttl)
            .field("agent_work_lease", &self.agent_work_lease)
            .field("ceremony_ttl", &self.ceremony_ttl)
            .field("email_verification_ttl", &self.email_verification_ttl)
            .field("account_recovery_ttl", &self.account_recovery_ttl)
            .field("email_outbox_key", &self.email_outbox_key)
            .field("archive_signing_key", &self.archive_signing_key)
            .field("archive_signing_key_id", &self.archive_signing_key_id)
            .field("metrics_token", &self.metrics_token)
            .field(
                "auth_rate_limit_per_minute",
                &self.auth_rate_limit_per_minute,
            )
            .field(
                "recovery_rate_limit_per_minute",
                &self.recovery_rate_limit_per_minute,
            )
            .field(
                "session_rate_limit_per_minute",
                &self.session_rate_limit_per_minute,
            )
            .field("webauthn_rp_id", &self.webauthn_rp_id)
            .field("webauthn_rp_name", &self.webauthn_rp_name)
            .field("deployment_environment", &self.deployment_environment)
            .field(
                "enable_experimental_crypto_for_development",
                &self.enable_experimental_crypto_for_development,
            )
            .finish()
    }
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let base_url = required("SPROUT_BASE_URL")?.parse::<Url>()?;
        let database_url = SecretString::new("DATABASE_URL", required("DATABASE_URL")?)?;
        let bind_addr = parse_or("SPROUT_BIND_ADDR", "127.0.0.1:8080")?;
        let database_max_connections = parse_or("SPROUT_DATABASE_MAX_CONNECTIONS", "10")?;
        let body_limit_bytes: usize =
            parse_or("SPROUT_BODY_LIMIT_BYTES", &DEFAULT_BODY_LIMIT.to_string())?;
        let blob_max_file_bytes =
            parse_or("SPROUT_BLOB_MAX_FILE_BYTES", &body_limit_bytes.to_string())?;
        let blob_project_quota_bytes = parse_or(
            "SPROUT_BLOB_PROJECT_QUOTA_BYTES",
            &DEFAULT_BLOB_PROJECT_QUOTA.to_string(),
        )?;
        let session_ttl_seconds: u64 = parse_or("SPROUT_SESSION_TTL_SECONDS", "86400")?;
        let agent_work_lease_seconds: u64 = parse_or("SPROUT_AGENT_WORK_LEASE_SECONDS", "300")?;
        let ceremony_ttl_seconds: u64 = parse_or("SPROUT_CEREMONY_TTL_SECONDS", "300")?;
        let email_verification_ttl_seconds: u64 =
            parse_or("SPROUT_EMAIL_VERIFICATION_TTL_SECONDS", "1800")?;
        let account_recovery_ttl_seconds: u64 =
            parse_or("SPROUT_ACCOUNT_RECOVERY_TTL_SECONDS", "900")?;
        let email_outbox_key = SecretKey::from_base64(
            "SPROUT_EMAIL_OUTBOX_KEY",
            &required("SPROUT_EMAIL_OUTBOX_KEY")?,
        )?;
        let archive_signing_key = SecretKey::from_base64(
            "SPROUT_ARCHIVE_SIGNING_KEY",
            &required("SPROUT_ARCHIVE_SIGNING_KEY")?,
        )?;
        let archive_signing_key_id = parse_required::<Uuid>("SPROUT_ARCHIVE_SIGNING_KEY_ID")?;
        let metrics_token = env::var("SPROUT_METRICS_TOKEN")
            .ok()
            .map(|value| SecretString::new("SPROUT_METRICS_TOKEN", value))
            .transpose()?;
        let auth_rate_limit_per_minute = parse_or(
            "SPROUT_AUTH_RATE_LIMIT_PER_MINUTE",
            &DEFAULT_AUTH_RATE_LIMIT_PER_MINUTE.to_string(),
        )?;
        let recovery_rate_limit_per_minute = parse_or(
            "SPROUT_RECOVERY_RATE_LIMIT_PER_MINUTE",
            &DEFAULT_RECOVERY_RATE_LIMIT_PER_MINUTE.to_string(),
        )?;
        let session_rate_limit_per_minute = parse_or(
            "SPROUT_SESSION_RATE_LIMIT_PER_MINUTE",
            &DEFAULT_SESSION_RATE_LIMIT_PER_MINUTE.to_string(),
        )?;
        let migrations_dir = env::var_os("SPROUT_MIGRATIONS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("db/migrations"));
        let blob_dir = env::var_os("SPROUT_BLOB_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("var/blobs"));
        let archive_dir = env::var_os("SPROUT_ARCHIVE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("var/archives"));
        let webauthn_rp_id = env::var("SPROUT_WEBAUTHN_RP_ID")
            .ok()
            .or_else(|| base_url.domain().map(ToOwned::to_owned))
            .ok_or(ConfigError::Invalid("SPROUT_WEBAUTHN_RP_ID"))?;
        let webauthn_rp_name =
            env::var("SPROUT_WEBAUTHN_RP_NAME").unwrap_or_else(|_| "Sprout".to_owned());
        let deployment_environment = parse_or("SPROUT_ENVIRONMENT", "production")?;
        let enable_experimental_crypto_for_development =
            parse_or("SPROUT_ENABLE_EXPERIMENTAL_CRYPTO_FOR_DEVELOPMENT", "false")?;
        let cors_origins = parse_origins(
            &env::var("SPROUT_CORS_ORIGINS")
                .unwrap_or_else(|_| base_url.origin().ascii_serialization()),
        )?;

        let config = Self {
            bind_addr,
            base_url,
            database_url,
            database_max_connections,
            migrations_dir,
            blob_dir,
            archive_dir,
            body_limit_bytes,
            blob_max_file_bytes,
            blob_project_quota_bytes,
            cors_origins,
            session_ttl: Duration::from_secs(session_ttl_seconds),
            agent_work_lease: Duration::from_secs(agent_work_lease_seconds),
            ceremony_ttl: Duration::from_secs(ceremony_ttl_seconds),
            email_verification_ttl: Duration::from_secs(email_verification_ttl_seconds),
            account_recovery_ttl: Duration::from_secs(account_recovery_ttl_seconds),
            email_outbox_key,
            archive_signing_key,
            archive_signing_key_id,
            metrics_token,
            auth_rate_limit_per_minute,
            recovery_rate_limit_per_minute,
            session_rate_limit_per_minute,
            webauthn_rp_id,
            webauthn_rp_name,
            deployment_environment,
            enable_experimental_crypto_for_development,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        self.validate_crypto_runtime_gate()?;
        if self.database_max_connections == 0 {
            return Err(ConfigError::Invalid("SPROUT_DATABASE_MAX_CONNECTIONS"));
        }
        if !(1024..=64 * 1024 * 1024).contains(&self.body_limit_bytes) {
            return Err(ConfigError::Invalid("SPROUT_BODY_LIMIT_BYTES"));
        }
        if self.blob_max_file_bytes == 0
            || self.blob_max_file_bytes > self.body_limit_bytes as u64
            || self.blob_project_quota_bytes < self.blob_max_file_bytes
        {
            return Err(ConfigError::Invalid("blob quota"));
        }
        if self.session_ttl.is_zero()
            || self.ceremony_ttl.is_zero()
            || self.email_verification_ttl.is_zero()
            || self.account_recovery_ttl.is_zero()
        {
            return Err(ConfigError::Invalid("authentication TTL"));
        }
        if self.agent_work_lease.is_zero() {
            return Err(ConfigError::Invalid("SPROUT_AGENT_WORK_LEASE_SECONDS"));
        }
        if self.archive_signing_key_id.is_nil() {
            return Err(ConfigError::Invalid("SPROUT_ARCHIVE_SIGNING_KEY_ID"));
        }
        if self.auth_rate_limit_per_minute == 0
            || self.recovery_rate_limit_per_minute == 0
            || self.session_rate_limit_per_minute == 0
        {
            return Err(ConfigError::Invalid("rate limit"));
        }
        if self.cors_origins.is_empty() {
            return Err(ConfigError::Invalid("SPROUT_CORS_ORIGINS"));
        }
        for origin in &self.cors_origins {
            validate_origin(origin)?;
        }
        validate_origin(&self.base_url)?;
        let host = self
            .base_url
            .host_str()
            .ok_or(ConfigError::Invalid("SPROUT_BASE_URL"))?;
        if host != self.webauthn_rp_id && !host.ends_with(&format!(".{}", self.webauthn_rp_id)) {
            return Err(ConfigError::Invalid("SPROUT_WEBAUTHN_RP_ID"));
        }
        Ok(())
    }

    fn validate_crypto_runtime_gate(&self) -> Result<(), ConfigError> {
        if self.deployment_environment == DeploymentEnvironment::Production
            && !INDEPENDENT_CRYPTO_AUDIT_COMPLETE
        {
            return Err(ConfigError::ProductionCryptoAuditRequired);
        }
        if !self.enable_experimental_crypto_for_development {
            return Err(ConfigError::ExperimentalCryptoDevelopmentOptInRequired);
        }
        Ok(())
    }

    #[doc(hidden)]
    pub fn for_test() -> Self {
        Self {
            bind_addr: "127.0.0.1:0".parse().expect("valid test address"),
            base_url: Url::parse("http://localhost:8080").expect("valid test URL"),
            database_url: SecretString::new(
                "DATABASE_URL",
                "postgresql://localhost/sprout_test".into(),
            )
            .expect("valid test database URL"),
            database_max_connections: 1,
            migrations_dir: PathBuf::from("../../db/migrations"),
            blob_dir: std::env::temp_dir().join("sprout-server-tests"),
            archive_dir: std::env::temp_dir().join("sprout-server-archive-tests"),
            body_limit_bytes: 1024,
            blob_max_file_bytes: 1024,
            blob_project_quota_bytes: 4096,
            cors_origins: vec![Url::parse("http://localhost:3000").expect("valid origin")],
            session_ttl: Duration::from_secs(3600),
            agent_work_lease: Duration::from_secs(300),
            ceremony_ttl: Duration::from_secs(300),
            email_verification_ttl: Duration::from_secs(1800),
            account_recovery_ttl: Duration::from_secs(900),
            email_outbox_key: SecretKey([7; 32]),
            archive_signing_key: SecretKey([8; 32]),
            archive_signing_key_id: Uuid::from_u128(8),
            metrics_token: Some(
                SecretString::new("SPROUT_METRICS_TOKEN", "test-metrics-token".into())
                    .expect("valid metrics token"),
            ),
            auth_rate_limit_per_minute: 30,
            recovery_rate_limit_per_minute: 10,
            session_rate_limit_per_minute: 600,
            webauthn_rp_id: "localhost".into(),
            webauthn_rp_name: "Sprout test".into(),
            deployment_environment: DeploymentEnvironment::Development,
            enable_experimental_crypto_for_development: true,
        }
    }
}

#[derive(Clone)]
pub struct SecretString(String);

impl SecretString {
    fn new(name: &'static str, value: String) -> Result<Self, ConfigError> {
        if value.trim().is_empty() {
            Err(ConfigError::Invalid(name))
        } else {
            Ok(Self(value))
        }
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

#[derive(Clone)]
pub struct SecretKey([u8; 32]);

impl SecretKey {
    fn from_base64(name: &'static str, value: &str) -> Result<Self, ConfigError> {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(value)
            .map_err(|_| ConfigError::Invalid(name))?;
        let bytes = decoded.try_into().map_err(|_| ConfigError::Invalid(name))?;
        Ok(Self(bytes))
    }

    pub fn expose(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

fn required(name: &'static str) -> Result<String, ConfigError> {
    env::var(name).map_err(|_| ConfigError::Missing(name))
}

fn parse_or<T>(name: &'static str, default: &str) -> Result<T, ConfigError>
where
    T: FromStr,
{
    env::var(name)
        .unwrap_or_else(|_| default.to_owned())
        .parse()
        .map_err(|_| ConfigError::Invalid(name))
}

fn parse_required<T>(name: &'static str) -> Result<T, ConfigError>
where
    T: FromStr,
{
    required(name)?
        .parse()
        .map_err(|_| ConfigError::Invalid(name))
}

fn parse_origins(value: &str) -> Result<Vec<Url>, ConfigError> {
    value
        .split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .map(|origin| Url::parse(origin).map_err(ConfigError::Url))
        .collect()
}

fn validate_origin(origin: &Url) -> Result<(), ConfigError> {
    if origin.cannot_be_a_base()
        || origin.username() != ""
        || origin.password().is_some()
        || origin.path() != "/"
        || origin.query().is_some()
        || origin.fragment().is_some()
    {
        return Err(ConfigError::Invalid("origin URL"));
    }
    if origin.scheme() == "https" || is_loopback(origin.host_str()) {
        Ok(())
    } else {
        Err(ConfigError::InsecureOrigin)
    }
}

fn is_loopback(host: Option<&str>) -> bool {
    matches!(host, Some("localhost"))
        || host
            .and_then(|value| value.parse::<IpAddr>().ok())
            .is_some_and(|address| address.is_loopback())
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("required environment variable {0} is missing")]
    Missing(&'static str),
    #[error("invalid value for {0}")]
    Invalid(&'static str),
    #[error("invalid URL: {0}")]
    Url(#[from] url::ParseError),
    #[error("non-loopback origins must use HTTPS")]
    InsecureOrigin,
    #[error("production use is blocked pending an independent cryptographic audit")]
    ProductionCryptoAuditRequired,
    #[error("the experimental crypto suite requires explicit development-only enablement")]
    ExperimentalCryptoDevelopmentOptInRequired,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_database_credentials() {
        let config = Config::for_test();
        let debug = format!("{config:?}");
        assert!(!debug.contains("postgresql://"));
        assert!(!debug.contains("test-metrics-token"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn rejects_insecure_non_loopback_origin() {
        let mut config = Config::for_test();
        config.cors_origins = vec![Url::parse("http://example.com").unwrap()];
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InsecureOrigin)
        ));
    }

    #[test]
    fn production_crypto_gate_remains_closed_even_with_development_opt_in() {
        let mut config = Config::for_test();
        config.deployment_environment = DeploymentEnvironment::Production;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::ProductionCryptoAuditRequired)
        ));
    }

    #[test]
    fn development_crypto_gate_requires_explicit_opt_in() {
        let mut config = Config::for_test();
        config.enable_experimental_crypto_for_development = false;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::ExperimentalCryptoDevelopmentOptInRequired)
        ));
    }

    #[test]
    fn agent_work_lease_must_be_nonzero() {
        let mut config = Config::for_test();
        config.agent_work_lease = Duration::ZERO;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Invalid("SPROUT_AGENT_WORK_LEASE_SECONDS"))
        ));
    }
}
