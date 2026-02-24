pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Starting,
    Healthy,
    Degraded,
    Stopping,
}

#[derive(Debug, Clone)]
pub enum ControlRequest {
    Status,
}

#[derive(Debug, Clone)]
pub struct StatusResponse {
    pub protocol_version: u16,
    pub health: HealthStatus,
    pub in_flight_requests: usize,
}

impl StatusResponse {
    #[must_use]
    pub fn healthy() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            health: HealthStatus::Healthy,
            in_flight_requests: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HealthStatus, PROTOCOL_VERSION, StatusResponse};

    #[test]
    fn healthy_response_uses_protocol_version() {
        let response = StatusResponse::healthy();
        assert_eq!(response.protocol_version, PROTOCOL_VERSION);
        assert_eq!(response.health, HealthStatus::Healthy);
        assert_eq!(response.in_flight_requests, 0);
    }
}
