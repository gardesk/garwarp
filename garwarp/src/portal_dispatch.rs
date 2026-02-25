#![allow(dead_code)]

use std::time::Duration;

use crate::error::PortalError;
use crate::request::{RequestError, RequestOwner, RequestRecord, RequestRegistry, RequestState};
use crate::validate::validate_request_identity;
use crate::window::parse_optional_parent_window;

#[derive(Debug)]
pub struct PortalDispatch {
    requests: RequestRegistry,
}

impl PortalDispatch {
    #[must_use]
    pub fn new(request_timeout: Duration) -> Self {
        Self {
            requests: RequestRegistry::new(request_timeout),
        }
    }

    pub fn register_unimplemented_call(
        &mut self,
        request_id: &str,
        sender: &str,
        app_id: &str,
        parent_window: &str,
    ) -> Result<(), PortalError> {
        let app_id = normalize_optional(app_id);
        validate_request_identity(request_id, sender, app_id)?;

        let parent_window = parse_optional_parent_window(normalize_optional(parent_window))
            .map_err(|_| PortalError::InvalidParentWindow)?;
        let owner = RequestOwner::new(sender.to_string(), app_id.map(str::to_string));
        let state = self.begin_request(request_id, owner.clone(), parent_window)?;

        if !state.is_terminal() {
            self.requests
                .transition(request_id, &owner, RequestState::Failed)
                .map_err(|error| map_request_error_to_portal(&error))?;
        }

        Ok(())
    }

    pub fn validate_update_choices(
        &self,
        request_id: &str,
        sender: &str,
    ) -> Result<(), PortalError> {
        validate_request_identity(request_id, sender, None)?;

        let record = self
            .requests
            .record(request_id)
            .ok_or(PortalError::RequestNotFound)?;
        if record.owner.sender != sender {
            return Err(PortalError::OwnershipMismatch);
        }
        if record.state.is_terminal() {
            return Err(PortalError::InvalidTransition);
        }
        Ok(())
    }

    #[must_use]
    pub fn record(&self, request_id: &str) -> Option<RequestRecord> {
        self.requests.record(request_id)
    }

    fn begin_request(
        &mut self,
        request_id: &str,
        owner: RequestOwner,
        parent_window: Option<crate::window::ParentWindowContext>,
    ) -> Result<RequestState, PortalError> {
        match self
            .requests
            .begin(request_id.to_string(), owner.clone(), parent_window)
        {
            Ok(()) => Ok(RequestState::Pending),
            Err(RequestError::AlreadyExists(_)) => match self.requests.record(request_id) {
                Some(record) if record.owner == owner && record.parent_window == parent_window => {
                    Ok(record.state)
                }
                Some(record) if record.owner != owner => Err(PortalError::OwnershipMismatch),
                _ => Err(PortalError::RequestAlreadyExists),
            },
            Err(error) => Err(map_request_error_to_portal(&error)),
        }
    }
}

fn normalize_optional(input: &str) -> Option<&str> {
    let trimmed = input.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn map_request_error_to_portal(error: &RequestError) -> PortalError {
    match error {
        RequestError::AlreadyExists(_) => PortalError::RequestAlreadyExists,
        RequestError::NotFound(_) => PortalError::RequestNotFound,
        RequestError::OwnerMismatch(_) => PortalError::OwnershipMismatch,
        RequestError::InvalidTransition { .. } => PortalError::InvalidTransition,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::PortalDispatch;
    use crate::error::PortalError;
    use crate::request::RequestState;
    use crate::window::ParentWindowContext;

    #[test]
    fn register_unimplemented_call_transitions_request_to_failed() {
        let mut dispatch = new_dispatch();
        dispatch
            .register_unimplemented_call("req:1_42:token_1", ":1.42", "org.test.App", "x11:0x2a")
            .expect("register request");

        let record = dispatch
            .record("req:1_42:token_1")
            .expect("record should exist");
        assert_eq!(record.state, RequestState::Failed);
        assert_eq!(record.owner.sender, ":1.42");
        assert_eq!(record.owner.app_id.as_deref(), Some("org.test.App"));
        assert_eq!(
            record.parent_window,
            Some(ParentWindowContext::X11 { window_id: 42 })
        );
    }

    #[test]
    fn register_unimplemented_call_is_idempotent_for_same_identity() {
        let mut dispatch = new_dispatch();
        dispatch
            .register_unimplemented_call("req:1_42:token_1", ":1.42", "org.test.App", "")
            .expect("first register");
        dispatch
            .register_unimplemented_call("req:1_42:token_1", ":1.42", "org.test.App", "")
            .expect("second register");

        let record = dispatch
            .record("req:1_42:token_1")
            .expect("record should exist");
        assert_eq!(record.state, RequestState::Failed);
    }

    #[test]
    fn register_unimplemented_call_rejects_owner_mismatch() {
        let mut dispatch = new_dispatch();
        dispatch
            .register_unimplemented_call("req:1_42:token_1", ":1.42", "org.test.App", "")
            .expect("first register");

        let result =
            dispatch.register_unimplemented_call("req:1_42:token_1", ":1.43", "org.test.App", "");
        assert_eq!(result, Err(PortalError::OwnershipMismatch));
    }

    #[test]
    fn register_unimplemented_call_rejects_invalid_parent_window() {
        let mut dispatch = new_dispatch();
        let result = dispatch.register_unimplemented_call(
            "req:1_42:token_1",
            ":1.42",
            "org.test.App",
            "wayland:surface",
        );
        assert_eq!(result, Err(PortalError::InvalidParentWindow));
    }

    #[test]
    fn update_choices_rejects_terminal_request() {
        let mut dispatch = new_dispatch();
        dispatch
            .register_unimplemented_call("req:1_42:token_1", ":1.42", "org.test.App", "")
            .expect("register request");

        let result = dispatch.validate_update_choices("req:1_42:token_1", ":1.42");
        assert_eq!(result, Err(PortalError::InvalidTransition));
    }

    fn new_dispatch() -> PortalDispatch {
        PortalDispatch::new(Duration::from_secs(30))
    }
}
