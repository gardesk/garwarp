use std::fmt;

pub const PROTOCOL_VERSION: u16 = 1;
pub const DEFAULT_RUNTIME_SUBDIR: &str = "garwarp";
pub const DEFAULT_CONTROL_SOCKET: &str = "control.sock";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Starting,
    Healthy,
    Degraded,
    Stopping,
}

impl HealthStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::Stopping => "stopping",
        }
    }

    fn parse(input: &str) -> Option<Self> {
        match input {
            "starting" => Some(Self::Starting),
            "healthy" => Some(Self::Healthy),
            "degraded" => Some(Self::Degraded),
            "stopping" => Some(Self::Stopping),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlRequest {
    Status,
    Stop,
}

impl ControlRequest {
    #[must_use]
    pub fn as_line(&self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Stop => "stop",
        }
    }

    #[must_use]
    pub fn parse_line(input: &str) -> Option<Self> {
        match input.trim() {
            "status" => Some(Self::Status),
            "stop" => Some(Self::Stop),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlResponse {
    Status(StatusResponse),
    AckStopping,
    Error { reason: String },
}

impl ControlResponse {
    #[must_use]
    pub fn to_line(&self) -> String {
        match self {
            Self::Status(status) => format!(
                "status protocol={} health={} in_flight={}\n",
                status.protocol_version,
                status.health.as_str(),
                status.in_flight_requests
            ),
            Self::AckStopping => "ack stopping\n".to_string(),
            Self::Error { reason } => format!("error reason={}\n", reason),
        }
    }

    pub fn parse_line(input: &str) -> Result<Self, ParseError> {
        let trimmed = input.trim();
        let mut parts = trimmed.split_whitespace();

        match parts.next() {
            Some("status") => {
                let mut protocol_version = None;
                let mut health = None;
                let mut in_flight_requests = None;

                for part in parts {
                    let (key, value) = part
                        .split_once('=')
                        .ok_or(ParseError::InvalidField(part.to_string()))?;
                    match key {
                        "protocol" => {
                            protocol_version = Some(
                                value
                                    .parse::<u16>()
                                    .map_err(|_| ParseError::InvalidField(part.to_string()))?,
                            );
                        }
                        "health" => {
                            health = HealthStatus::parse(value);
                            if health.is_none() {
                                return Err(ParseError::InvalidField(part.to_string()));
                            }
                        }
                        "in_flight" => {
                            in_flight_requests = Some(
                                value
                                    .parse::<usize>()
                                    .map_err(|_| ParseError::InvalidField(part.to_string()))?,
                            );
                        }
                        _ => return Err(ParseError::InvalidField(part.to_string())),
                    }
                }

                let status = StatusResponse {
                    protocol_version: protocol_version
                        .ok_or(ParseError::MissingField("protocol"))?,
                    health: health.ok_or(ParseError::MissingField("health"))?,
                    in_flight_requests: in_flight_requests
                        .ok_or(ParseError::MissingField("in_flight"))?,
                };
                Ok(Self::Status(status))
            }
            Some("ack") => match parts.next() {
                Some("stopping") => Ok(Self::AckStopping),
                Some(other) => Err(ParseError::UnknownToken(other.to_string())),
                None => Err(ParseError::MissingField("ack")),
            },
            Some("error") => match parts.next() {
                Some(reason_field) => {
                    let (key, value) = reason_field
                        .split_once('=')
                        .ok_or(ParseError::InvalidField(reason_field.to_string()))?;
                    if key != "reason" {
                        return Err(ParseError::InvalidField(reason_field.to_string()));
                    }
                    Ok(Self::Error {
                        reason: value.to_string(),
                    })
                }
                None => Err(ParseError::MissingField("reason")),
            },
            Some(other) => Err(ParseError::UnknownToken(other.to_string())),
            None => Err(ParseError::Empty),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Empty,
    MissingField(&'static str),
    InvalidField(String),
    UnknownToken(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "empty input"),
            Self::MissingField(field) => write!(f, "missing field: {field}"),
            Self::InvalidField(field) => write!(f, "invalid field: {field}"),
            Self::UnknownToken(token) => write!(f, "unknown token: {token}"),
        }
    }
}

impl std::error::Error for ParseError {}

#[cfg(test)]
mod tests {
    use super::{ControlRequest, ControlResponse, HealthStatus, PROTOCOL_VERSION, StatusResponse};

    #[test]
    fn request_parse_roundtrip() {
        for request in [ControlRequest::Status, ControlRequest::Stop] {
            let line = request.as_line();
            let parsed = ControlRequest::parse_line(line);
            assert_eq!(parsed, Some(request));
        }
    }

    #[test]
    fn response_status_roundtrip() {
        let response = ControlResponse::Status(StatusResponse {
            protocol_version: PROTOCOL_VERSION,
            health: HealthStatus::Healthy,
            in_flight_requests: 7,
        });
        let line = response.to_line();
        let parsed = ControlResponse::parse_line(&line).expect("response should parse");
        assert_eq!(parsed, response);
    }

    #[test]
    fn response_ack_roundtrip() {
        let response = ControlResponse::AckStopping;
        let line = response.to_line();
        let parsed = ControlResponse::parse_line(&line).expect("response should parse");
        assert_eq!(parsed, response);
    }

    #[test]
    fn healthy_response_uses_protocol_version() {
        let response = StatusResponse::healthy();
        assert_eq!(response.protocol_version, PROTOCOL_VERSION);
        assert_eq!(response.health, HealthStatus::Healthy);
        assert_eq!(response.in_flight_requests, 0);
    }

    #[test]
    fn malformed_status_is_rejected() {
        let parsed = ControlResponse::parse_line("status protocol=one health=healthy in_flight=0");
        assert!(parsed.is_err());
    }
}
