use crate::error::PortalError;

const MAX_REQUEST_ID_LEN: usize = 128;
const MAX_SENDER_LEN: usize = 128;
const MAX_APP_ID_LEN: usize = 255;

pub fn validate_request_identity(
    request_id: &str,
    sender: &str,
    app_id: Option<&str>,
) -> Result<(), PortalError> {
    if !is_valid_request_id(request_id) {
        return Err(PortalError::InvalidRequestPayload);
    }
    if !is_valid_sender(sender) {
        return Err(PortalError::InvalidRequestPayload);
    }
    if let Some(app_id) = app_id
        && !is_valid_app_id(app_id)
    {
        return Err(PortalError::InvalidRequestPayload);
    }
    Ok(())
}

fn is_valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_REQUEST_ID_LEN
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | ':'))
}

fn is_valid_sender(value: &str) -> bool {
    value.len() <= MAX_SENDER_LEN
        && value.starts_with(':')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | ':'))
}

fn is_valid_app_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_APP_ID_LEN
        && value.contains('.')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::validate_request_identity;

    #[test]
    fn accepts_valid_identity() {
        let result = validate_request_identity("req-1", ":1.2", Some("org.test.App"));
        assert!(result.is_ok());
    }

    #[test]
    fn rejects_invalid_request_id() {
        let result = validate_request_identity("req/1", ":1.2", Some("org.test.App"));
        assert!(result.is_err());
    }

    #[test]
    fn rejects_invalid_sender() {
        let result = validate_request_identity("req-1", "org.test.App", Some("org.test.App"));
        assert!(result.is_err());
    }

    #[test]
    fn rejects_invalid_app_id() {
        let result = validate_request_identity("req-1", ":1.2", Some("bad$app"));
        assert!(result.is_err());
    }
}
