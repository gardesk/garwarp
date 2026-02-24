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
    ListRequests,
    InspectRequest {
        id: String,
    },
    BeginRequest {
        id: String,
        sender: String,
        app_id: Option<String>,
        parent_window: Option<String>,
    },
    TransitionRequest {
        id: String,
        sender: String,
        app_id: Option<String>,
        target: RequestTransitionTarget,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestTransitionTarget {
    AwaitingUser,
    Fulfilled,
    Cancelled,
    Failed,
}

impl RequestTransitionTarget {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AwaitingUser => "awaiting_user",
            Self::Fulfilled => "fulfilled",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }

    fn parse(input: &str) -> Option<Self> {
        match input {
            "awaiting_user" => Some(Self::AwaitingUser),
            "fulfilled" => Some(Self::Fulfilled),
            "cancelled" => Some(Self::Cancelled),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

impl ControlRequest {
    #[must_use]
    pub fn as_line(&self) -> String {
        match self {
            Self::Status => "status".to_string(),
            Self::Stop => "stop".to_string(),
            Self::ListRequests => "list".to_string(),
            Self::InspectRequest { id } => format!("inspect id={id}"),
            Self::BeginRequest {
                id,
                sender,
                app_id,
                parent_window,
            } => {
                let mut parts = vec![
                    "begin".to_string(),
                    format!("id={id}"),
                    format!("sender={sender}"),
                ];
                if let Some(app_id) = app_id {
                    parts.push(format!("app_id={app_id}"));
                }
                if let Some(parent_window) = parent_window {
                    parts.push(format!("parent={parent_window}"));
                }
                parts.join(" ")
            }
            Self::TransitionRequest {
                id,
                sender,
                app_id,
                target,
            } => {
                let mut parts = vec![
                    "transition".to_string(),
                    format!("id={id}"),
                    format!("sender={sender}"),
                    format!("state={}", target.as_str()),
                ];
                if let Some(app_id) = app_id {
                    parts.push(format!("app_id={app_id}"));
                }
                parts.join(" ")
            }
        }
    }

    #[must_use]
    pub fn parse_line(input: &str) -> Option<Self> {
        let trimmed = input.trim();
        if trimmed == "status" {
            return Some(Self::Status);
        }
        if trimmed == "stop" {
            return Some(Self::Stop);
        }
        if trimmed == "list" {
            return Some(Self::ListRequests);
        }

        let mut parts = trimmed.split_whitespace();
        match parts.next() {
            Some("inspect") => {
                let fields = parse_fields(parts)?;
                let id = fields.get("id")?.clone();
                Some(Self::InspectRequest { id })
            }
            Some("begin") => {
                let fields = parse_fields(parts)?;
                let id = fields.get("id")?.clone();
                let sender = fields.get("sender")?.clone();
                let app_id = fields.get("app_id").cloned();
                let parent_window = fields.get("parent").cloned();
                Some(Self::BeginRequest {
                    id,
                    sender,
                    app_id,
                    parent_window,
                })
            }
            Some("transition") => {
                let fields = parse_fields(parts)?;
                let id = fields.get("id")?.clone();
                let sender = fields.get("sender")?.clone();
                let app_id = fields.get("app_id").cloned();
                let target = RequestTransitionTarget::parse(fields.get("state")?)?;
                Some(Self::TransitionRequest {
                    id,
                    sender,
                    app_id,
                    target,
                })
            }
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
    RequestList {
        ids: Vec<String>,
    },
    AckRequest {
        id: String,
        state: String,
    },
    RequestSnapshot {
        id: String,
        state: String,
        sender: String,
        app_id: Option<String>,
        parent_window: Option<String>,
    },
    Error {
        code: u32,
        reason: String,
    },
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
            Self::RequestList { ids } => {
                let ids = if ids.is_empty() {
                    "-".to_string()
                } else {
                    ids.join(",")
                };
                format!("list ids={ids}\n")
            }
            Self::AckRequest { id, state } => {
                format!("ack request id={} state={}\n", id, state)
            }
            Self::RequestSnapshot {
                id,
                state,
                sender,
                app_id,
                parent_window,
            } => {
                let app_id = app_id.as_deref().unwrap_or("-");
                let parent_window = parent_window.as_deref().unwrap_or("-");
                format!(
                    "snapshot id={} state={} sender={} app_id={} parent={}\n",
                    id, state, sender, app_id, parent_window
                )
            }
            Self::Error { code, reason } => format!("error code={} reason={}\n", code, reason),
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
                Some("request") => {
                    let mut id = None;
                    let mut state = None;
                    for part in parts {
                        let (key, value) = part
                            .split_once('=')
                            .ok_or(ParseError::InvalidField(part.to_string()))?;
                        match key {
                            "id" => id = Some(value.to_string()),
                            "state" => state = Some(value.to_string()),
                            _ => return Err(ParseError::InvalidField(part.to_string())),
                        }
                    }
                    Ok(Self::AckRequest {
                        id: id.ok_or(ParseError::MissingField("id"))?,
                        state: state.ok_or(ParseError::MissingField("state"))?,
                    })
                }
                Some(other) => Err(ParseError::UnknownToken(other.to_string())),
                None => Err(ParseError::MissingField("ack")),
            },
            Some("list") => {
                let mut ids = None;
                for part in parts {
                    let (key, value) = part
                        .split_once('=')
                        .ok_or(ParseError::InvalidField(part.to_string()))?;
                    match key {
                        "ids" => {
                            if value == "-" {
                                ids = Some(Vec::new());
                            } else {
                                let parsed =
                                    value.split(',').map(str::to_string).collect::<Vec<_>>();
                                if parsed.iter().any(|id| id.is_empty()) {
                                    return Err(ParseError::InvalidField(part.to_string()));
                                }
                                ids = Some(parsed);
                            }
                        }
                        _ => return Err(ParseError::InvalidField(part.to_string())),
                    }
                }
                Ok(Self::RequestList {
                    ids: ids.ok_or(ParseError::MissingField("ids"))?,
                })
            }
            Some("snapshot") => {
                let mut id = None;
                let mut state = None;
                let mut sender = None;
                let mut app_id = None;
                let mut parent_window = None;

                for part in parts {
                    let (key, value) = part
                        .split_once('=')
                        .ok_or(ParseError::InvalidField(part.to_string()))?;
                    match key {
                        "id" => id = Some(value.to_string()),
                        "state" => state = Some(value.to_string()),
                        "sender" => sender = Some(value.to_string()),
                        "app_id" => {
                            if value != "-" {
                                app_id = Some(value.to_string());
                            }
                        }
                        "parent" => {
                            if value != "-" {
                                parent_window = Some(value.to_string());
                            }
                        }
                        _ => return Err(ParseError::InvalidField(part.to_string())),
                    }
                }

                Ok(Self::RequestSnapshot {
                    id: id.ok_or(ParseError::MissingField("id"))?,
                    state: state.ok_or(ParseError::MissingField("state"))?,
                    sender: sender.ok_or(ParseError::MissingField("sender"))?,
                    app_id,
                    parent_window,
                })
            }
            Some("error") => match parts.next() {
                Some(first_field) => {
                    let mut code = None;
                    let mut reason = None;
                    let mut fields = vec![first_field];
                    fields.extend(parts);

                    for field in fields {
                        let (key, value) = field
                            .split_once('=')
                            .ok_or(ParseError::InvalidField(field.to_string()))?;
                        match key {
                            "code" => {
                                code =
                                    Some(value.parse::<u32>().map_err(|_| {
                                        ParseError::InvalidField(field.to_string())
                                    })?);
                            }
                            "reason" => reason = Some(value.to_string()),
                            _ => return Err(ParseError::InvalidField(field.to_string())),
                        }
                    }
                    Ok(Self::Error {
                        code: code.ok_or(ParseError::MissingField("code"))?,
                        reason: reason.ok_or(ParseError::MissingField("reason"))?,
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

fn parse_fields<'a, I>(parts: I) -> Option<std::collections::HashMap<String, String>>
where
    I: Iterator<Item = &'a str>,
{
    let mut fields = std::collections::HashMap::new();
    for part in parts {
        let (key, value) = part.split_once('=')?;
        if fields.contains_key(key) {
            return None;
        }
        fields.insert(key.to_string(), value.to_string());
    }
    Some(fields)
}

#[cfg(test)]
mod tests {
    use super::{
        ControlRequest, ControlResponse, HealthStatus, PROTOCOL_VERSION, RequestTransitionTarget,
        StatusResponse,
    };

    #[test]
    fn request_parse_roundtrip() {
        for request in [
            ControlRequest::Status,
            ControlRequest::Stop,
            ControlRequest::ListRequests,
            ControlRequest::InspectRequest {
                id: "req-1".to_string(),
            },
            ControlRequest::BeginRequest {
                id: "req-1".to_string(),
                sender: ":1.2".to_string(),
                app_id: Some("org.test.App".to_string()),
                parent_window: Some("x11:0x2a".to_string()),
            },
            ControlRequest::TransitionRequest {
                id: "req-1".to_string(),
                sender: ":1.2".to_string(),
                app_id: Some("org.test.App".to_string()),
                target: RequestTransitionTarget::Cancelled,
            },
        ] {
            let line = request.as_line();
            let parsed = ControlRequest::parse_line(&line);
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
        for response in [
            ControlResponse::AckStopping,
            ControlResponse::RequestList {
                ids: vec!["req-1".to_string(), "req-2".to_string()],
            },
            ControlResponse::RequestList { ids: Vec::new() },
            ControlResponse::AckRequest {
                id: "req-1".to_string(),
                state: "pending".to_string(),
            },
            ControlResponse::RequestSnapshot {
                id: "req-1".to_string(),
                state: "awaiting_user".to_string(),
                sender: ":1.2".to_string(),
                app_id: Some("org.test.App".to_string()),
                parent_window: Some("x11:0x2a".to_string()),
            },
            ControlResponse::Error {
                code: 2,
                reason: "invalid_request".to_string(),
            },
        ] {
            let line = response.to_line();
            let parsed = ControlResponse::parse_line(&line).expect("response should parse");
            assert_eq!(parsed, response);
        }
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

    #[test]
    fn request_parse_rejects_duplicate_fields() {
        let parsed = ControlRequest::parse_line("inspect id=req-1 id=req-2");
        assert_eq!(parsed, None);
    }
}
