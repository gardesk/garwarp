#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, Instant};

use crate::window::ParentWindowContext;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestOwner {
    pub sender: String,
    pub app_id: Option<String>,
}

impl RequestOwner {
    #[must_use]
    pub fn new(sender: impl Into<String>, app_id: Option<String>) -> Self {
        Self {
            sender: sender.into(),
            app_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestState {
    Pending,
    AwaitingUser,
    Fulfilled,
    Cancelled,
    Failed,
    Expired,
}

impl RequestState {
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Fulfilled | Self::Cancelled | Self::Failed | Self::Expired
        )
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::AwaitingUser => "awaiting_user",
            Self::Fulfilled => "fulfilled",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Expired => "expired",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RequestEntry {
    pub id: String,
    pub owner: RequestOwner,
    pub parent_window: Option<ParentWindowContext>,
    pub state: RequestState,
    started_at: Instant,
    last_updated_at: Instant,
}

impl RequestEntry {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        owner: RequestOwner,
        parent_window: Option<ParentWindowContext>,
        now: Instant,
    ) -> Self {
        Self {
            id: id.into(),
            owner,
            parent_window,
            state: RequestState::Pending,
            started_at: now,
            last_updated_at: now,
        }
    }
}

#[derive(Debug)]
pub struct RequestRegistry {
    entries: HashMap<String, RequestEntry>,
    timeout: Duration,
}

impl RequestRegistry {
    #[must_use]
    pub fn new(timeout: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            timeout,
        }
    }

    pub fn begin(
        &mut self,
        id: impl Into<String>,
        owner: RequestOwner,
        parent_window: Option<ParentWindowContext>,
    ) -> Result<(), RequestError> {
        self.begin_at(id, owner, parent_window, Instant::now())
    }

    pub fn begin_at(
        &mut self,
        id: impl Into<String>,
        owner: RequestOwner,
        parent_window: Option<ParentWindowContext>,
        now: Instant,
    ) -> Result<(), RequestError> {
        let id = id.into();
        if self.entries.contains_key(&id) {
            return Err(RequestError::AlreadyExists(id));
        }
        self.entries
            .insert(id.clone(), RequestEntry::new(id, owner, parent_window, now));
        Ok(())
    }

    pub fn transition(
        &mut self,
        id: &str,
        owner: &RequestOwner,
        target: RequestState,
    ) -> Result<(), RequestError> {
        self.transition_at(id, owner, target, Instant::now())
    }

    pub fn transition_at(
        &mut self,
        id: &str,
        owner: &RequestOwner,
        target: RequestState,
        now: Instant,
    ) -> Result<(), RequestError> {
        let entry = self
            .entries
            .get_mut(id)
            .ok_or_else(|| RequestError::NotFound(id.to_string()))?;

        if entry.owner != *owner {
            return Err(RequestError::OwnerMismatch(id.to_string()));
        }

        if !is_valid_transition(entry.state, target) {
            return Err(RequestError::InvalidTransition {
                id: id.to_string(),
                from: entry.state,
                to: target,
            });
        }

        entry.state = target;
        entry.last_updated_at = now;
        Ok(())
    }

    pub fn expire_stale(&mut self, now: Instant) -> Vec<String> {
        let mut expired = Vec::new();
        for entry in self.entries.values_mut() {
            if entry.state.is_terminal() {
                continue;
            }
            if now.duration_since(entry.started_at) >= self.timeout {
                entry.state = RequestState::Expired;
                entry.last_updated_at = now;
                expired.push(entry.id.clone());
            }
        }
        expired
    }

    pub fn recover_after_restart(&mut self, now: Instant) -> Vec<String> {
        let mut expired = Vec::new();
        for entry in self.entries.values_mut() {
            if entry.state.is_terminal() {
                continue;
            }
            entry.state = RequestState::Expired;
            entry.last_updated_at = now;
            expired.push(entry.id.clone());
        }
        expired
    }

    #[must_use]
    pub fn in_flight_count(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| {
                matches!(
                    entry.state,
                    RequestState::Pending | RequestState::AwaitingUser
                )
            })
            .count()
    }

    #[must_use]
    pub fn state(&self, id: &str) -> Option<RequestState> {
        self.entries.get(id).map(|entry| entry.state)
    }

    #[must_use]
    pub fn parent_window(&self, id: &str) -> Option<Option<ParentWindowContext>> {
        self.entries.get(id).map(|entry| entry.parent_window)
    }
}

fn is_valid_transition(from: RequestState, to: RequestState) -> bool {
    use RequestState::{AwaitingUser, Cancelled, Expired, Failed, Fulfilled, Pending};
    matches!(
        (from, to),
        (Pending, AwaitingUser)
            | (Pending, Cancelled)
            | (Pending, Failed)
            | (Pending, Expired)
            | (AwaitingUser, Fulfilled)
            | (AwaitingUser, Cancelled)
            | (AwaitingUser, Failed)
            | (AwaitingUser, Expired)
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestError {
    AlreadyExists(String),
    NotFound(String),
    OwnerMismatch(String),
    InvalidTransition {
        id: String,
        from: RequestState,
        to: RequestState,
    },
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyExists(id) => write!(f, "request already exists: {id}"),
            Self::NotFound(id) => write!(f, "request not found: {id}"),
            Self::OwnerMismatch(id) => write!(f, "request ownership mismatch: {id}"),
            Self::InvalidTransition { id, from, to } => {
                write!(f, "invalid transition for {id}: {from:?} -> {to:?}")
            }
        }
    }
}

impl std::error::Error for RequestError {}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{RequestOwner, RequestRegistry, RequestState};
    use crate::window::ParentWindowContext;

    fn owner(sender: &str) -> RequestOwner {
        RequestOwner::new(sender, Some("org.test.App".to_string()))
    }

    #[test]
    fn cancel_during_awaiting_user() {
        let now = Instant::now();
        let mut registry = RequestRegistry::new(Duration::from_secs(5));
        registry
            .begin_at("req-1", owner(":1.2"), None, now)
            .expect("request should be created");
        registry
            .transition_at("req-1", &owner(":1.2"), RequestState::AwaitingUser, now)
            .expect("request should transition to awaiting user");
        registry
            .transition_at(
                "req-1",
                &owner(":1.2"),
                RequestState::Cancelled,
                now + Duration::from_millis(20),
            )
            .expect("request should be cancellable in awaiting user state");

        assert_eq!(registry.state("req-1"), Some(RequestState::Cancelled));
        assert_eq!(registry.in_flight_count(), 0);
    }

    #[test]
    fn timeout_expires_awaiting_request() {
        let now = Instant::now();
        let mut registry = RequestRegistry::new(Duration::from_millis(100));
        registry
            .begin_at("req-1", owner(":1.2"), None, now)
            .expect("request should be created");
        registry
            .transition_at("req-1", &owner(":1.2"), RequestState::AwaitingUser, now)
            .expect("request should transition to awaiting user");

        let expired = registry.expire_stale(now + Duration::from_millis(101));
        assert_eq!(expired, vec!["req-1".to_string()]);
        assert_eq!(registry.state("req-1"), Some(RequestState::Expired));
    }

    #[test]
    fn restart_recovery_expires_in_flight_requests() {
        let now = Instant::now();
        let mut registry = RequestRegistry::new(Duration::from_secs(10));
        registry
            .begin_at("req-pending", owner(":1.2"), None, now)
            .expect("request should be created");
        registry
            .begin_at("req-awaiting", owner(":1.3"), None, now)
            .expect("request should be created");
        registry
            .transition_at(
                "req-awaiting",
                &owner(":1.3"),
                RequestState::AwaitingUser,
                now + Duration::from_millis(1),
            )
            .expect("request should transition to awaiting user");
        registry
            .transition_at(
                "req-awaiting",
                &owner(":1.3"),
                RequestState::Cancelled,
                now + Duration::from_millis(2),
            )
            .expect("request should transition to cancelled");

        let expired = registry.recover_after_restart(now + Duration::from_secs(1));
        assert_eq!(expired, vec!["req-pending".to_string()]);
        assert_eq!(registry.state("req-pending"), Some(RequestState::Expired));
        assert_eq!(
            registry.state("req-awaiting"),
            Some(RequestState::Cancelled)
        );
    }

    #[test]
    fn owner_mismatch_is_rejected() {
        let now = Instant::now();
        let mut registry = RequestRegistry::new(Duration::from_secs(5));
        registry
            .begin_at("req-1", owner(":1.2"), None, now)
            .expect("request should be created");
        let result = registry.transition_at(
            "req-1",
            &owner(":1.7"),
            RequestState::AwaitingUser,
            now + Duration::from_millis(1),
        );
        assert!(result.is_err());
        assert_eq!(registry.state("req-1"), Some(RequestState::Pending));
    }

    #[test]
    fn begin_creates_pending_request() {
        let mut registry = RequestRegistry::new(Duration::from_secs(5));
        registry
            .begin("req-begin", owner(":1.4"), None)
            .expect("request should be created");
        assert_eq!(registry.state("req-begin"), Some(RequestState::Pending));
    }

    #[test]
    fn fulfilled_transition_is_allowed_from_awaiting() {
        let now = Instant::now();
        let mut registry = RequestRegistry::new(Duration::from_secs(5));
        let request_owner = owner(":1.2");
        registry
            .begin_at("req-1", request_owner.clone(), None, now)
            .expect("request should be created");
        registry
            .transition_at("req-1", &request_owner, RequestState::AwaitingUser, now)
            .expect("request should transition to awaiting user");
        registry
            .transition_at(
                "req-1",
                &request_owner,
                RequestState::Fulfilled,
                now + Duration::from_millis(10),
            )
            .expect("request should transition to fulfilled");
        assert_eq!(registry.state("req-1"), Some(RequestState::Fulfilled));
    }

    #[test]
    fn failed_transition_is_allowed_from_awaiting() {
        let now = Instant::now();
        let mut registry = RequestRegistry::new(Duration::from_secs(5));
        let request_owner = owner(":1.2");
        registry
            .begin_at("req-1", request_owner.clone(), None, now)
            .expect("request should be created");
        registry
            .transition_at("req-1", &request_owner, RequestState::AwaitingUser, now)
            .expect("request should transition to awaiting user");
        registry
            .transition_at(
                "req-1",
                &request_owner,
                RequestState::Failed,
                now + Duration::from_millis(10),
            )
            .expect("request should transition to failed");
        assert_eq!(registry.state("req-1"), Some(RequestState::Failed));
    }

    #[test]
    fn begin_records_parent_window_context() {
        let now = Instant::now();
        let mut registry = RequestRegistry::new(Duration::from_secs(5));
        registry
            .begin_at(
                "req-window",
                owner(":1.9"),
                Some(ParentWindowContext::X11 { window_id: 42 }),
                now,
            )
            .expect("request should be created");

        assert_eq!(
            registry.parent_window("req-window"),
            Some(Some(ParentWindowContext::X11 { window_id: 42 }))
        );
    }
}
