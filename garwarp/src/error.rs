#![allow(dead_code)]

use crate::request::RequestError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum PortalResponseCode {
    Success = 0,
    Cancelled = 1,
    Failed = 2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortalError {
    CancelledByUser,
    InvalidRequestPayload,
    InvalidParentWindow,
    OwnershipMismatch,
    RequestNotFound,
    RequestAlreadyExists,
    RequestTimeout,
    InvalidTransition,
    InternalFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorMapping {
    pub code: PortalResponseCode,
    pub reason: &'static str,
}

#[must_use]
pub fn map_portal_error(error: &PortalError) -> ErrorMapping {
    match error {
        PortalError::CancelledByUser => ErrorMapping {
            code: PortalResponseCode::Cancelled,
            reason: "cancelled",
        },
        PortalError::InvalidRequestPayload => ErrorMapping {
            code: PortalResponseCode::Failed,
            reason: "invalid_request",
        },
        PortalError::InvalidParentWindow => ErrorMapping {
            code: PortalResponseCode::Failed,
            reason: "invalid_parent_window",
        },
        PortalError::OwnershipMismatch => ErrorMapping {
            code: PortalResponseCode::Failed,
            reason: "ownership_mismatch",
        },
        PortalError::RequestNotFound => ErrorMapping {
            code: PortalResponseCode::Failed,
            reason: "request_not_found",
        },
        PortalError::RequestAlreadyExists => ErrorMapping {
            code: PortalResponseCode::Failed,
            reason: "request_conflict",
        },
        PortalError::RequestTimeout => ErrorMapping {
            code: PortalResponseCode::Failed,
            reason: "request_timeout",
        },
        PortalError::InvalidTransition => ErrorMapping {
            code: PortalResponseCode::Failed,
            reason: "invalid_transition",
        },
        PortalError::InternalFailure => ErrorMapping {
            code: PortalResponseCode::Failed,
            reason: "internal_error",
        },
    }
}

#[must_use]
pub fn map_request_error(error: &RequestError) -> ErrorMapping {
    match error {
        RequestError::AlreadyExists(_) => map_portal_error(&PortalError::RequestAlreadyExists),
        RequestError::NotFound(_) => map_portal_error(&PortalError::RequestNotFound),
        RequestError::OwnerMismatch(_) => map_portal_error(&PortalError::OwnershipMismatch),
        RequestError::InvalidTransition { .. } => map_portal_error(&PortalError::InvalidTransition),
    }
}

#[cfg(test)]
mod tests {
    use super::{PortalError, PortalResponseCode, map_portal_error, map_request_error};
    use crate::request::{RequestError, RequestState};

    #[test]
    fn cancelled_maps_to_cancelled_code() {
        let mapping = map_portal_error(&PortalError::CancelledByUser);
        assert_eq!(mapping.code, PortalResponseCode::Cancelled);
        assert_eq!(mapping.reason, "cancelled");
        assert_eq!(mapping.code as u32, 1);
    }

    #[test]
    fn request_owner_mismatch_maps_to_failed_code() {
        let mapping = map_request_error(&RequestError::OwnerMismatch("req-1".to_string()));
        assert_eq!(mapping.code, PortalResponseCode::Failed);
        assert_eq!(mapping.reason, "ownership_mismatch");
        assert_eq!(mapping.code as u32, 2);
    }

    #[test]
    fn invalid_transition_maps_to_stable_reason() {
        let mapping = map_request_error(&RequestError::InvalidTransition {
            id: "req-1".to_string(),
            from: RequestState::Pending,
            to: RequestState::Fulfilled,
        });
        assert_eq!(mapping.code, PortalResponseCode::Failed);
        assert_eq!(mapping.reason, "invalid_transition");
    }

    #[test]
    fn all_portal_error_variants_are_mapped() {
        let errors = [
            PortalError::CancelledByUser,
            PortalError::InvalidRequestPayload,
            PortalError::InvalidParentWindow,
            PortalError::OwnershipMismatch,
            PortalError::RequestNotFound,
            PortalError::RequestAlreadyExists,
            PortalError::RequestTimeout,
            PortalError::InvalidTransition,
            PortalError::InternalFailure,
        ];

        for error in errors {
            let mapping = map_portal_error(&error);
            assert!(!mapping.reason.is_empty());
        }
    }

    #[test]
    fn success_code_constant_stays_zero() {
        assert_eq!(PortalResponseCode::Success as u32, 0);
    }
}
