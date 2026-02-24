#![allow(dead_code)]

use crate::error::PortalError;
use crate::validate::validate_request_id;

pub const REQUEST_HANDLE_PREFIX: &str = "/org/freedesktop/portal/desktop/request/";
const REQUEST_ID_PREFIX: &str = "req:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestHandle {
    pub sender_segment: String,
    pub token: String,
}

pub fn parse_request_handle(handle: &str) -> Result<RequestHandle, PortalError> {
    if handle.trim() != handle {
        return Err(PortalError::InvalidRequestPayload);
    }

    let suffix = handle
        .strip_prefix(REQUEST_HANDLE_PREFIX)
        .ok_or(PortalError::InvalidRequestPayload)?;

    let mut parts = suffix.split('/');
    let sender_segment = parts.next().ok_or(PortalError::InvalidRequestPayload)?;
    let token = parts.next().ok_or(PortalError::InvalidRequestPayload)?;

    if sender_segment.is_empty() || token.is_empty() || parts.next().is_some() {
        return Err(PortalError::InvalidRequestPayload);
    }
    if !is_valid_path_segment(sender_segment) || !is_valid_path_segment(token) {
        return Err(PortalError::InvalidRequestPayload);
    }

    Ok(RequestHandle {
        sender_segment: sender_segment.to_string(),
        token: token.to_string(),
    })
}

pub fn sender_to_handle_segment(sender: &str) -> Result<String, PortalError> {
    if sender.trim() != sender {
        return Err(PortalError::InvalidRequestPayload);
    }

    let unique_body = sender
        .strip_prefix(':')
        .ok_or(PortalError::InvalidRequestPayload)?;

    if unique_body.is_empty() {
        return Err(PortalError::InvalidRequestPayload);
    }

    let mut segment = String::with_capacity(unique_body.len());
    for ch in unique_body.chars() {
        if ch.is_ascii_alphanumeric() {
            segment.push(ch);
            continue;
        }
        if matches!(ch, '.' | '_' | '-') {
            segment.push('_');
            continue;
        }
        return Err(PortalError::InvalidRequestPayload);
    }

    if segment.is_empty() || !is_valid_path_segment(&segment) {
        return Err(PortalError::InvalidRequestPayload);
    }

    Ok(segment)
}

pub fn derive_request_id(
    caller_sender: &str,
    handle: &RequestHandle,
) -> Result<String, PortalError> {
    let expected_sender_segment = sender_to_handle_segment(caller_sender)?;
    if expected_sender_segment != handle.sender_segment {
        return Err(PortalError::OwnershipMismatch);
    }

    let request_id = format!(
        "{REQUEST_ID_PREFIX}{expected_sender_segment}:{}",
        handle.token
    );
    validate_request_id(&request_id)?;
    Ok(request_id)
}

pub fn derive_request_id_from_handle(
    caller_sender: &str,
    request_handle_path: &str,
) -> Result<String, PortalError> {
    let request_handle = parse_request_handle(request_handle_path)?;
    derive_request_id(caller_sender, &request_handle)
}

fn is_valid_path_segment(value: &str) -> bool {
    value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

#[cfg(test)]
mod tests {
    use super::{
        REQUEST_HANDLE_PREFIX, RequestHandle, derive_request_id, derive_request_id_from_handle,
        parse_request_handle, sender_to_handle_segment,
    };
    use crate::error::PortalError;

    #[test]
    fn parse_valid_request_handle() {
        let handle = format!("{REQUEST_HANDLE_PREFIX}1_42/token_1");
        let parsed = parse_request_handle(&handle).expect("request handle should parse");
        assert_eq!(
            parsed,
            RequestHandle {
                sender_segment: "1_42".to_string(),
                token: "token_1".to_string(),
            }
        );
    }

    #[test]
    fn parse_rejects_invalid_prefix() {
        let parsed = parse_request_handle("/org/example/request/1_2/token");
        assert!(parsed.is_err());
    }

    #[test]
    fn parse_rejects_whitespace_wrapped_input() {
        let handle = format!("  {REQUEST_HANDLE_PREFIX}1_42/token_1");
        let parsed = parse_request_handle(&handle);
        assert_eq!(parsed, Err(PortalError::InvalidRequestPayload));
    }

    #[test]
    fn parse_rejects_missing_segments() {
        let parsed = parse_request_handle(&format!("{REQUEST_HANDLE_PREFIX}only_sender"));
        assert!(parsed.is_err());
    }

    #[test]
    fn parse_rejects_extra_path_segments() {
        let parsed = parse_request_handle(&format!("{REQUEST_HANDLE_PREFIX}1_42/token_1/extra"));
        assert_eq!(parsed, Err(PortalError::InvalidRequestPayload));
    }

    #[test]
    fn parse_rejects_invalid_segment_characters() {
        let parsed = parse_request_handle(&format!("{REQUEST_HANDLE_PREFIX}1.42/token_1"));
        assert_eq!(parsed, Err(PortalError::InvalidRequestPayload));
    }

    #[test]
    fn sender_to_handle_segment_normalizes_unique_name() {
        let segment = sender_to_handle_segment(":1.420-a").expect("sender segment");
        assert_eq!(segment, "1_420_a");
    }

    #[test]
    fn sender_to_handle_segment_rejects_invalid_sender() {
        let segment = sender_to_handle_segment("org.example.App");
        assert_eq!(segment, Err(PortalError::InvalidRequestPayload));
    }

    #[test]
    fn derive_request_id_is_deterministic_and_valid() {
        let handle = RequestHandle {
            sender_segment: "1_42".to_string(),
            token: "token_1".to_string(),
        };
        let id = derive_request_id(":1.42", &handle).expect("request id");
        assert_eq!(id, "req:1_42:token_1");
    }

    #[test]
    fn derive_request_id_rejects_sender_mismatch() {
        let handle = RequestHandle {
            sender_segment: "1_42".to_string(),
            token: "token_1".to_string(),
        };
        let id = derive_request_id(":1.43", &handle);
        assert_eq!(id, Err(PortalError::OwnershipMismatch));
    }

    #[test]
    fn derive_request_id_rejects_when_composed_id_is_invalid() {
        let handle = RequestHandle {
            sender_segment: "1_42".to_string(),
            token: "x".repeat(200),
        };
        let id = derive_request_id(":1.42", &handle);
        assert_eq!(id, Err(PortalError::InvalidRequestPayload));
    }

    #[test]
    fn derive_request_id_from_handle_parses_and_derives() {
        let handle = format!("{REQUEST_HANDLE_PREFIX}1_42/token_1");
        let id = derive_request_id_from_handle(":1.42", &handle).expect("derived request id");
        assert_eq!(id, "req:1_42:token_1");
    }
}
