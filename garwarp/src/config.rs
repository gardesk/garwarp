use std::env;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Config {
    pub poll_interval: Duration,
    pub request_timeout: Duration,
    pub terminal_retention: Duration,
}

impl Config {
    #[must_use]
    pub fn from_env() -> Self {
        const DEFAULT_POLL_MS: u64 = 100;
        const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 30_000;
        const DEFAULT_TERMINAL_RETENTION_MS: u64 = 30_000;
        let poll_interval = env::var("GARWARP_POLL_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .map(Duration::from_millis)
            .unwrap_or_else(|| Duration::from_millis(DEFAULT_POLL_MS));
        let request_timeout = env::var("GARWARP_REQUEST_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .map(Duration::from_millis)
            .unwrap_or_else(|| Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS));
        let terminal_retention = env::var("GARWARP_TERMINAL_RETENTION_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .map(Duration::from_millis)
            .unwrap_or_else(|| Duration::from_millis(DEFAULT_TERMINAL_RETENTION_MS));
        Self {
            poll_interval,
            request_timeout,
            terminal_retention,
        }
    }
}
