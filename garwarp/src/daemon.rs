use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use garwarp_ipc::{
    ControlRequest, ControlResponse, HealthStatus, RequestTransitionTarget, StatusResponse,
};

use crate::config::Config;
use crate::dbus::{self, SessionNameGuard};
use crate::error::{PortalError, map_portal_error, map_request_error};
use crate::lock::SingleInstanceGuard;
use crate::logging;
use crate::request::{RequestError, RequestOwner, RequestRegistry, RequestState};
use crate::request_store;
use crate::runtime::RuntimePaths;
use crate::validate::{validate_request_id, validate_request_identity};
use crate::window::parse_optional_parent_window;

const MAX_CONTROL_LINE_BYTES: usize = 4096;

pub fn run() -> io::Result<()> {
    let config = Config::from_env();
    let paths = RuntimePaths::from_env();
    paths.ensure_runtime_dir()?;

    let _lock = SingleInstanceGuard::acquire(&paths.lock_file)?;
    remove_stale_socket(&paths.control_socket)?;
    let _dbus_guard = acquire_dbus_name()?;

    let listener = UnixListener::bind(&paths.control_socket)?;
    listener.set_nonblocking(true)?;

    logging::info("daemon_starting");

    let (requests, recovered_ids, startup_degraded) =
        load_registry_with_fallback(&paths.request_store, config.request_timeout);
    if !recovered_ids.is_empty() {
        logging::warn(&format!(
            "request_recovery_expired count={}",
            recovered_ids.len()
        ));
    }

    let mut state = DaemonState {
        health: if startup_degraded {
            HealthStatus::Degraded
        } else {
            HealthStatus::Healthy
        },
        requests,
        running: true,
    };
    persist_registry_state(&paths.request_store, &state.requests);

    while state.running {
        let expired = state.requests.expire_stale(Instant::now());
        for id in &expired {
            logging::warn(&format!("request_expired id={id}"));
        }
        if !expired.is_empty() {
            persist_registry_state(&paths.request_store, &state.requests);
        }

        let pruned = state
            .requests
            .prune_terminal(Instant::now(), config.terminal_retention);
        for id in &pruned {
            logging::info(&format!("request_pruned id={id}"));
        }
        if !pruned.is_empty() {
            persist_registry_state(&paths.request_store, &state.requests);
        }

        match listener.accept() {
            Ok((stream, _address)) => {
                if let Err(error) = handle_connection(stream, &mut state) {
                    logging::warn(&format!("request_error={error}"));
                } else {
                    persist_registry_state(&paths.request_store, &state.requests);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(config.poll_interval);
            }
            Err(error) => {
                logging::error(&format!("accept_error={error}"));
                thread::sleep(config.poll_interval);
            }
        }
    }

    let _ = fs::remove_file(&paths.control_socket);
    logging::info("daemon_stopped");
    Ok(())
}

fn acquire_dbus_name() -> io::Result<SessionNameGuard> {
    SessionNameGuard::acquire().map_err(|error| {
        io::Error::other(format!(
            "failed to claim dbus name {}: {error}",
            dbus::BACKEND_DBUS_NAME
        ))
    })
}

#[derive(Debug)]
struct DaemonState {
    health: HealthStatus,
    requests: RequestRegistry,
    running: bool,
}

fn handle_connection(stream: UnixStream, state: &mut DaemonState) -> io::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let bytes_read = {
        let mut limited = reader.by_ref().take((MAX_CONTROL_LINE_BYTES + 1) as u64);
        limited.read_line(&mut line)?
    };

    if bytes_read > MAX_CONTROL_LINE_BYTES {
        let mapping = map_portal_error(&PortalError::InvalidRequestPayload);
        return write_response(
            reader.into_inner(),
            ControlResponse::Error {
                code: mapping.code as u32,
                reason: mapping.reason.to_string(),
            },
        );
    }

    let response = match ControlRequest::parse_line(&line) {
        Some(ControlRequest::Status) => ControlResponse::Status(StatusResponse {
            protocol_version: garwarp_ipc::PROTOCOL_VERSION,
            health: state.health,
            in_flight_requests: state.requests.in_flight_count(),
            total_requests: state.requests.total_count(),
            terminal_requests: state.requests.terminal_count(),
        }),
        Some(ControlRequest::Stop) => {
            state.health = HealthStatus::Stopping;
            state.running = false;
            ControlResponse::AckStopping
        }
        Some(ControlRequest::ListRequests) => {
            let ids = state
                .requests
                .records()
                .into_iter()
                .map(|record| record.id)
                .collect::<Vec<_>>();
            ControlResponse::RequestList { ids }
        }
        Some(ControlRequest::InspectRequest { id }) => {
            let validation = validate_request_id(&id);
            if let Err(error) = validation {
                let mapping = map_portal_error(&error);
                return write_response(
                    reader.into_inner(),
                    ControlResponse::Error {
                        code: mapping.code as u32,
                        reason: mapping.reason.to_string(),
                    },
                );
            }

            match state.requests.record(&id) {
                Some(record) => ControlResponse::RequestSnapshot {
                    id: record.id,
                    state: record.state.as_str().to_string(),
                    sender: record.owner.sender,
                    app_id: record.owner.app_id,
                    parent_window: record.parent_window.map(|parent| parent.as_str()),
                },
                None => {
                    let mapping = map_portal_error(&PortalError::RequestNotFound);
                    ControlResponse::Error {
                        code: mapping.code as u32,
                        reason: mapping.reason.to_string(),
                    }
                }
            }
        }
        Some(ControlRequest::BeginRequest {
            id,
            sender: _sender,
            app_id,
            parent_window,
        }) => {
            let sender = match trusted_sender(reader.get_ref()) {
                Ok(sender) => sender,
                Err(error) => {
                    let mapping = map_portal_error(&PortalError::InternalFailure);
                    logging::warn(&format!("peer_identity_error={error}"));
                    return write_response(
                        reader.into_inner(),
                        ControlResponse::Error {
                            code: mapping.code as u32,
                            reason: mapping.reason.to_string(),
                        },
                    );
                }
            };

            let validation = validate_request_identity(&id, &sender, app_id.as_deref());
            if let Err(error) = validation {
                let mapping = map_portal_error(&error);
                return write_response(
                    reader.into_inner(),
                    ControlResponse::Error {
                        code: mapping.code as u32,
                        reason: mapping.reason.to_string(),
                    },
                );
            }

            let owner = RequestOwner::new(sender, app_id);
            let parsed_parent_window = match parse_optional_parent_window(parent_window.as_deref())
            {
                Ok(parent_window) => parent_window,
                Err(_) => {
                    let mapping = map_portal_error(&PortalError::InvalidParentWindow);
                    return write_response(
                        reader.into_inner(),
                        ControlResponse::Error {
                            code: mapping.code as u32,
                            reason: mapping.reason.to_string(),
                        },
                    );
                }
            };

            match state
                .requests
                .begin(id.clone(), owner.clone(), parsed_parent_window)
            {
                Ok(()) => ControlResponse::AckRequest {
                    id,
                    state: "pending".to_string(),
                },
                Err(RequestError::AlreadyExists(_)) => {
                    let existing = state.requests.record(&id);
                    match existing {
                        Some(record)
                            if record.owner == owner
                                && record.parent_window == parsed_parent_window =>
                        {
                            ControlResponse::AckRequest {
                                id,
                                state: record.state.as_str().to_string(),
                            }
                        }
                        Some(record) if record.owner != owner => {
                            let mapping = map_portal_error(&PortalError::OwnershipMismatch);
                            ControlResponse::Error {
                                code: mapping.code as u32,
                                reason: mapping.reason.to_string(),
                            }
                        }
                        _ => {
                            let mapping = map_portal_error(&PortalError::RequestAlreadyExists);
                            ControlResponse::Error {
                                code: mapping.code as u32,
                                reason: mapping.reason.to_string(),
                            }
                        }
                    }
                }
                Err(error) => {
                    let mapping = map_request_error(&error);
                    ControlResponse::Error {
                        code: mapping.code as u32,
                        reason: mapping.reason.to_string(),
                    }
                }
            }
        }
        Some(ControlRequest::TransitionRequest {
            id,
            sender: _sender,
            app_id,
            target,
        }) => {
            let sender = match trusted_sender(reader.get_ref()) {
                Ok(sender) => sender,
                Err(error) => {
                    let mapping = map_portal_error(&PortalError::InternalFailure);
                    logging::warn(&format!("peer_identity_error={error}"));
                    return write_response(
                        reader.into_inner(),
                        ControlResponse::Error {
                            code: mapping.code as u32,
                            reason: mapping.reason.to_string(),
                        },
                    );
                }
            };

            let validation = validate_request_identity(&id, &sender, app_id.as_deref());
            if let Err(error) = validation {
                let mapping = map_portal_error(&error);
                return write_response(
                    reader.into_inner(),
                    ControlResponse::Error {
                        code: mapping.code as u32,
                        reason: mapping.reason.to_string(),
                    },
                );
            }

            let owner = RequestOwner::new(sender, app_id);
            let target_state = map_transition_target(target);
            match state.requests.transition(&id, &owner, target_state) {
                Ok(()) => ControlResponse::AckRequest {
                    id,
                    state: target_state.as_str().to_string(),
                },
                Err(error) => {
                    let mapping = map_request_error(&error);
                    ControlResponse::Error {
                        code: mapping.code as u32,
                        reason: mapping.reason.to_string(),
                    }
                }
            }
        }
        None => {
            let mapping = map_portal_error(&PortalError::InvalidRequestPayload);
            ControlResponse::Error {
                code: mapping.code as u32,
                reason: mapping.reason.to_string(),
            }
        }
    };

    let stream = reader.into_inner();
    write_response(stream, response)
}

fn write_response(mut stream: UnixStream, response: ControlResponse) -> io::Result<()> {
    stream.write_all(response.to_line().as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn remove_stale_socket(path: &std::path::Path) -> io::Result<()> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn map_transition_target(target: RequestTransitionTarget) -> RequestState {
    match target {
        RequestTransitionTarget::AwaitingUser => RequestState::AwaitingUser,
        RequestTransitionTarget::Fulfilled => RequestState::Fulfilled,
        RequestTransitionTarget::Cancelled => RequestState::Cancelled,
        RequestTransitionTarget::Failed => RequestState::Failed,
    }
}

fn load_registry_with_recovery(
    request_store_path: &std::path::Path,
    timeout: Duration,
) -> io::Result<(RequestRegistry, Vec<String>)> {
    let mut registry = request_store::load_registry(request_store_path, timeout)?;
    let expired = registry.recover_after_restart(Instant::now());
    Ok((registry, expired))
}

fn load_registry_with_fallback(
    request_store_path: &Path,
    timeout: Duration,
) -> (RequestRegistry, Vec<String>, bool) {
    match load_registry_with_recovery(request_store_path, timeout) {
        Ok((registry, recovered_ids)) => (registry, recovered_ids, false),
        Err(error) => {
            logging::warn(&format!("request_store_load_failed error={error}"));
            match quarantine_request_store(request_store_path) {
                Ok(Some(path)) => logging::warn(&format!(
                    "request_store_quarantined path={}",
                    path.display()
                )),
                Ok(None) => {}
                Err(error) => {
                    logging::warn(&format!("request_store_quarantine_failed error={error}"))
                }
            }
            (RequestRegistry::new(timeout), Vec::new(), true)
        }
    }
}

fn quarantine_request_store(path: &Path) -> io::Result<Option<std::path::PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("requests.state");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());

    let quarantined = parent.join(format!("{file_name}.corrupt-{nanos}"));
    fs::rename(path, &quarantined)?;
    Ok(Some(quarantined))
}

fn trusted_sender(stream: &UnixStream) -> io::Result<String> {
    #[cfg(target_os = "linux")]
    {
        use std::mem;
        use std::os::fd::AsRawFd;

        let fd = stream.as_raw_fd();
        let mut cred = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut len = mem::size_of::<libc::ucred>() as libc::socklen_t;
        let rc = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut cred as *mut libc::ucred).cast::<libc::c_void>(),
                &mut len,
            )
        };
        if rc == -1 {
            return Err(io::Error::last_os_error());
        }
        if len as usize != mem::size_of::<libc::ucred>() {
            return Err(io::Error::other("invalid peer credential size"));
        }
        return Ok(canonical_sender_for_uid(cred.uid));
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = stream;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "peer credentials are unsupported on this platform",
        ))
    }
}

fn canonical_sender_for_uid(uid: u32) -> String {
    format!(":uid.{uid}")
}

fn persist_registry_state(path: &std::path::Path, registry: &RequestRegistry) {
    if let Err(error) = request_store::persist_registry(path, registry) {
        logging::warn(&format!("request_store_write_failed error={error}"));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DaemonState, MAX_CONTROL_LINE_BYTES, canonical_sender_for_uid, handle_connection,
        load_registry_with_fallback, load_registry_with_recovery,
    };
    use garwarp_ipc::{ControlResponse, HealthStatus};
    use std::fs;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use crate::request::{RequestOwner, RequestRegistry, RequestState};
    use crate::request_store;
    use crate::window::ParentWindowContext;

    fn unique_temp_file() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        std::env::temp_dir().join(format!("garwarp-daemon-recovery-{nanos}.state"))
    }

    fn local_sender() -> String {
        canonical_sender_for_uid(unsafe { libc::geteuid() })
    }

    #[test]
    fn status_request_returns_status_response() {
        let (mut client, server) = UnixStream::pair().expect("pair should be created");
        client
            .write_all(b"status\n")
            .expect("status request should be written");

        let mut state = DaemonState {
            health: HealthStatus::Healthy,
            requests: RequestRegistry::new(Duration::from_secs(5)),
            running: true,
        };
        state
            .requests
            .begin_at(
                "req-1",
                RequestOwner::new(local_sender(), None),
                None,
                Instant::now(),
            )
            .expect("request should be created");
        state
            .requests
            .transition(
                "req-1",
                &RequestOwner::new(local_sender(), None),
                RequestState::AwaitingUser,
            )
            .expect("request should transition");
        handle_connection(server, &mut state).expect("status should be handled");

        let mut response_line = String::new();
        let mut reader = BufReader::new(client);
        reader
            .read_line(&mut response_line)
            .expect("response should be readable");

        let response = ControlResponse::parse_line(&response_line).expect("response should parse");
        match response {
            ControlResponse::Status(status) => {
                assert_eq!(status.health, HealthStatus::Healthy);
                assert_eq!(status.in_flight_requests, 1);
                assert_eq!(status.total_requests, 1);
                assert_eq!(status.terminal_requests, 0);
            }
            _ => panic!("expected status response"),
        }
        assert!(state.running);
    }

    #[test]
    fn stop_request_flips_running_state() {
        let (mut client, server) = UnixStream::pair().expect("pair should be created");
        client
            .write_all(b"stop\n")
            .expect("stop request should be written");

        let mut state = DaemonState {
            health: HealthStatus::Healthy,
            requests: RequestRegistry::new(Duration::from_secs(5)),
            running: true,
        };
        handle_connection(server, &mut state).expect("stop should be handled");

        let mut response_line = String::new();
        let mut reader = BufReader::new(client);
        reader
            .read_line(&mut response_line)
            .expect("response should be readable");
        let response = ControlResponse::parse_line(&response_line).expect("response should parse");
        assert_eq!(response, ControlResponse::AckStopping);
        assert_eq!(state.health, HealthStatus::Stopping);
        assert!(!state.running);
    }

    #[test]
    fn list_requests_returns_sorted_ids() {
        let (mut client, server) = UnixStream::pair().expect("pair should be created");
        client
            .write_all(b"list\n")
            .expect("list request should be written");

        let mut state = DaemonState {
            health: HealthStatus::Healthy,
            requests: RequestRegistry::new(Duration::from_secs(5)),
            running: true,
        };
        state
            .requests
            .begin_at(
                "req-b",
                RequestOwner::new(":1.2", None),
                None,
                Instant::now(),
            )
            .expect("request should be created");
        state
            .requests
            .begin_at(
                "req-a",
                RequestOwner::new(":1.3", None),
                None,
                Instant::now(),
            )
            .expect("request should be created");

        handle_connection(server, &mut state).expect("list should be handled");

        let mut response_line = String::new();
        let mut reader = BufReader::new(client);
        reader
            .read_line(&mut response_line)
            .expect("response should be readable");
        let response = ControlResponse::parse_line(&response_line).expect("response should parse");
        assert_eq!(
            response,
            ControlResponse::RequestList {
                ids: vec!["req-a".to_string(), "req-b".to_string()],
            }
        );
    }

    #[test]
    fn invalid_request_uses_stable_error_reason() {
        let (mut client, server) = UnixStream::pair().expect("pair should be created");
        client
            .write_all(b"unknown\n")
            .expect("invalid request should be written");

        let mut state = DaemonState {
            health: HealthStatus::Healthy,
            requests: RequestRegistry::new(Duration::from_secs(5)),
            running: true,
        };
        handle_connection(server, &mut state).expect("request should be handled");

        let mut response_line = String::new();
        let mut reader = BufReader::new(client);
        reader
            .read_line(&mut response_line)
            .expect("response should be readable");
        let response = ControlResponse::parse_line(&response_line).expect("response should parse");
        assert_eq!(
            response,
            ControlResponse::Error {
                code: 2,
                reason: "invalid_request".to_string(),
            }
        );
    }

    #[test]
    fn oversized_request_maps_to_invalid_request() {
        let (mut client, server) = UnixStream::pair().expect("pair should be created");
        let oversized = format!("{}\n", "x".repeat(MAX_CONTROL_LINE_BYTES + 1));
        client
            .write_all(oversized.as_bytes())
            .expect("oversized request should be written");

        let mut state = DaemonState {
            health: HealthStatus::Healthy,
            requests: RequestRegistry::new(Duration::from_secs(5)),
            running: true,
        };
        handle_connection(server, &mut state).expect("request should be handled");

        let mut response_line = String::new();
        let mut reader = BufReader::new(client);
        reader
            .read_line(&mut response_line)
            .expect("response should be readable");
        let response = ControlResponse::parse_line(&response_line).expect("response should parse");
        assert_eq!(
            response,
            ControlResponse::Error {
                code: 2,
                reason: "invalid_request".to_string(),
            }
        );
    }

    #[test]
    fn duplicate_request_fields_map_to_invalid_request() {
        let (mut client, server) = UnixStream::pair().expect("pair should be created");
        client
            .write_all(b"inspect id=req-1 id=req-2\n")
            .expect("inspect request should be written");

        let mut state = DaemonState {
            health: HealthStatus::Healthy,
            requests: RequestRegistry::new(Duration::from_secs(5)),
            running: true,
        };
        handle_connection(server, &mut state).expect("request should be handled");

        let mut response_line = String::new();
        let mut reader = BufReader::new(client);
        reader
            .read_line(&mut response_line)
            .expect("response should be readable");
        let response = ControlResponse::parse_line(&response_line).expect("response should parse");
        assert_eq!(
            response,
            ControlResponse::Error {
                code: 2,
                reason: "invalid_request".to_string(),
            }
        );
    }

    #[test]
    fn unknown_request_fields_map_to_invalid_request() {
        let (mut client, server) = UnixStream::pair().expect("pair should be created");
        client
            .write_all(b"begin id=req-1 sender=:1.2 bogus=1\n")
            .expect("begin request should be written");

        let mut state = DaemonState {
            health: HealthStatus::Healthy,
            requests: RequestRegistry::new(Duration::from_secs(5)),
            running: true,
        };
        handle_connection(server, &mut state).expect("request should be handled");

        let mut response_line = String::new();
        let mut reader = BufReader::new(client);
        reader
            .read_line(&mut response_line)
            .expect("response should be readable");
        let response = ControlResponse::parse_line(&response_line).expect("response should parse");
        assert_eq!(
            response,
            ControlResponse::Error {
                code: 2,
                reason: "invalid_request".to_string(),
            }
        );
    }

    #[test]
    fn begin_request_tracks_parent_window_context() {
        let (mut client, server) = UnixStream::pair().expect("pair should be created");
        client
            .write_all(b"begin id=req-1 sender=:1.2 parent=x11:0x2a\n")
            .expect("begin request should be written");

        let mut state = DaemonState {
            health: HealthStatus::Healthy,
            requests: RequestRegistry::new(Duration::from_secs(5)),
            running: true,
        };
        handle_connection(server, &mut state).expect("begin should be handled");

        let mut response_line = String::new();
        let mut reader = BufReader::new(client);
        reader
            .read_line(&mut response_line)
            .expect("response should be readable");

        let response = ControlResponse::parse_line(&response_line).expect("response should parse");
        assert_eq!(
            response,
            ControlResponse::AckRequest {
                id: "req-1".to_string(),
                state: "pending".to_string(),
            }
        );
        assert_eq!(
            state.requests.parent_window("req-1"),
            Some(Some(ParentWindowContext::X11 { window_id: 42 }))
        );
        assert_eq!(state.requests.in_flight_count(), 1);
    }

    #[test]
    fn duplicate_begin_with_same_owner_is_idempotent() {
        let (mut client, server) = UnixStream::pair().expect("pair should be created");
        client
            .write_all(b"begin id=req-1 sender=:1.2 parent=x11:0x2a\n")
            .expect("begin request should be written");

        let mut state = DaemonState {
            health: HealthStatus::Healthy,
            requests: RequestRegistry::new(Duration::from_secs(5)),
            running: true,
        };
        state
            .requests
            .begin_at(
                "req-1",
                RequestOwner::new(local_sender(), None),
                Some(ParentWindowContext::X11 { window_id: 42 }),
                Instant::now(),
            )
            .expect("request should be created");
        state
            .requests
            .transition(
                "req-1",
                &RequestOwner::new(local_sender(), None),
                RequestState::AwaitingUser,
            )
            .expect("request should transition");

        handle_connection(server, &mut state).expect("begin should be handled");

        let mut response_line = String::new();
        let mut reader = BufReader::new(client);
        reader
            .read_line(&mut response_line)
            .expect("response should be readable");
        let response = ControlResponse::parse_line(&response_line).expect("response should parse");
        assert_eq!(
            response,
            ControlResponse::AckRequest {
                id: "req-1".to_string(),
                state: "awaiting_user".to_string(),
            }
        );
    }

    #[test]
    fn duplicate_begin_with_different_owner_maps_to_ownership_mismatch() {
        let (mut client, server) = UnixStream::pair().expect("pair should be created");
        client
            .write_all(b"begin id=req-1 sender=:1.7 parent=x11:0x2a\n")
            .expect("begin request should be written");

        let mut state = DaemonState {
            health: HealthStatus::Healthy,
            requests: RequestRegistry::new(Duration::from_secs(5)),
            running: true,
        };
        state
            .requests
            .begin_at(
                "req-1",
                RequestOwner::new(":uid.4242", None),
                Some(ParentWindowContext::X11 { window_id: 42 }),
                Instant::now(),
            )
            .expect("request should be created");
        handle_connection(server, &mut state).expect("begin should be handled");

        let mut response_line = String::new();
        let mut reader = BufReader::new(client);
        reader
            .read_line(&mut response_line)
            .expect("response should be readable");
        let response = ControlResponse::parse_line(&response_line).expect("response should parse");
        assert_eq!(
            response,
            ControlResponse::Error {
                code: 2,
                reason: "ownership_mismatch".to_string(),
            }
        );
    }

    #[test]
    fn invalid_parent_window_maps_to_stable_reason() {
        let (mut client, server) = UnixStream::pair().expect("pair should be created");
        client
            .write_all(b"begin id=req-1 sender=:1.2 parent=wayland:abc\n")
            .expect("begin request should be written");

        let mut state = DaemonState {
            health: HealthStatus::Healthy,
            requests: RequestRegistry::new(Duration::from_secs(5)),
            running: true,
        };
        handle_connection(server, &mut state).expect("begin should be handled");

        let mut response_line = String::new();
        let mut reader = BufReader::new(client);
        reader
            .read_line(&mut response_line)
            .expect("response should be readable");
        let response = ControlResponse::parse_line(&response_line).expect("response should parse");
        assert_eq!(
            response,
            ControlResponse::Error {
                code: 2,
                reason: "invalid_parent_window".to_string(),
            }
        );
    }

    #[test]
    fn invalid_request_id_maps_to_invalid_request() {
        let (mut client, server) = UnixStream::pair().expect("pair should be created");
        client
            .write_all(b"begin id=req/1 sender=:1.2 parent=x11:0x2a\n")
            .expect("begin request should be written");

        let mut state = DaemonState {
            health: HealthStatus::Healthy,
            requests: RequestRegistry::new(Duration::from_secs(5)),
            running: true,
        };
        handle_connection(server, &mut state).expect("begin should be handled");

        let mut response_line = String::new();
        let mut reader = BufReader::new(client);
        reader
            .read_line(&mut response_line)
            .expect("response should be readable");
        let response = ControlResponse::parse_line(&response_line).expect("response should parse");
        assert_eq!(
            response,
            ControlResponse::Error {
                code: 2,
                reason: "invalid_request".to_string(),
            }
        );
    }

    #[test]
    fn payload_sender_is_ignored_for_begin() {
        let (mut client, server) = UnixStream::pair().expect("pair should be created");
        client
            .write_all(b"begin id=req-1 sender=org.test.App parent=x11:0x2a\n")
            .expect("begin request should be written");

        let mut state = DaemonState {
            health: HealthStatus::Healthy,
            requests: RequestRegistry::new(Duration::from_secs(5)),
            running: true,
        };
        handle_connection(server, &mut state).expect("begin should be handled");

        let mut response_line = String::new();
        let mut reader = BufReader::new(client);
        reader
            .read_line(&mut response_line)
            .expect("response should be readable");
        let response = ControlResponse::parse_line(&response_line).expect("response should parse");
        assert_eq!(
            response,
            ControlResponse::AckRequest {
                id: "req-1".to_string(),
                state: "pending".to_string(),
            }
        );
        assert_eq!(
            state.requests.owner("req-1").map(|owner| owner.sender),
            Some(local_sender())
        );
    }

    #[test]
    fn transition_owner_mismatch_maps_to_stable_reason() {
        let (mut client, server) = UnixStream::pair().expect("pair should be created");
        client
            .write_all(b"transition id=req-1 sender=:1.7 state=cancelled\n")
            .expect("transition request should be written");

        let mut state = DaemonState {
            health: HealthStatus::Healthy,
            requests: RequestRegistry::new(Duration::from_secs(5)),
            running: true,
        };
        state
            .requests
            .begin_at(
                "req-1",
                RequestOwner::new(":uid.4242", None),
                Some(ParentWindowContext::X11 { window_id: 42 }),
                Instant::now(),
            )
            .expect("request should be created");
        handle_connection(server, &mut state).expect("transition should be handled");

        let mut response_line = String::new();
        let mut reader = BufReader::new(client);
        reader
            .read_line(&mut response_line)
            .expect("response should be readable");
        let response = ControlResponse::parse_line(&response_line).expect("response should parse");
        assert_eq!(
            response,
            ControlResponse::Error {
                code: 2,
                reason: "ownership_mismatch".to_string(),
            }
        );
        assert_eq!(state.requests.state("req-1"), Some(RequestState::Pending));
    }

    #[test]
    fn duplicate_cancel_returns_ack() {
        let (mut client, server) = UnixStream::pair().expect("pair should be created");
        client
            .write_all(b"transition id=req-1 sender=:1.2 state=cancelled\n")
            .expect("transition request should be written");

        let mut state = DaemonState {
            health: HealthStatus::Healthy,
            requests: RequestRegistry::new(Duration::from_secs(5)),
            running: true,
        };
        state
            .requests
            .begin_at(
                "req-1",
                RequestOwner::new(local_sender(), None),
                None,
                Instant::now(),
            )
            .expect("request should be created");
        state
            .requests
            .transition(
                "req-1",
                &RequestOwner::new(local_sender(), None),
                RequestState::Cancelled,
            )
            .expect("first cancel should transition");
        handle_connection(server, &mut state).expect("transition should be handled");

        let mut response_line = String::new();
        let mut reader = BufReader::new(client);
        reader
            .read_line(&mut response_line)
            .expect("response should be readable");
        let response = ControlResponse::parse_line(&response_line).expect("response should parse");
        assert_eq!(
            response,
            ControlResponse::AckRequest {
                id: "req-1".to_string(),
                state: "cancelled".to_string(),
            }
        );
        assert_eq!(state.requests.state("req-1"), Some(RequestState::Cancelled));
    }

    #[test]
    fn duplicate_awaiting_user_returns_ack() {
        let (mut client, server) = UnixStream::pair().expect("pair should be created");
        client
            .write_all(b"transition id=req-1 sender=:1.2 state=awaiting_user\n")
            .expect("transition request should be written");

        let mut state = DaemonState {
            health: HealthStatus::Healthy,
            requests: RequestRegistry::new(Duration::from_secs(5)),
            running: true,
        };
        state
            .requests
            .begin_at(
                "req-1",
                RequestOwner::new(local_sender(), None),
                None,
                Instant::now(),
            )
            .expect("request should be created");
        state
            .requests
            .transition(
                "req-1",
                &RequestOwner::new(local_sender(), None),
                RequestState::AwaitingUser,
            )
            .expect("first awaiting_user should transition");
        handle_connection(server, &mut state).expect("transition should be handled");

        let mut response_line = String::new();
        let mut reader = BufReader::new(client);
        reader
            .read_line(&mut response_line)
            .expect("response should be readable");
        let response = ControlResponse::parse_line(&response_line).expect("response should parse");
        assert_eq!(
            response,
            ControlResponse::AckRequest {
                id: "req-1".to_string(),
                state: "awaiting_user".to_string(),
            }
        );
        assert_eq!(
            state.requests.state("req-1"),
            Some(RequestState::AwaitingUser)
        );
    }

    #[test]
    fn inspect_returns_request_snapshot() {
        let (mut client, server) = UnixStream::pair().expect("pair should be created");
        client
            .write_all(b"inspect id=req-1\n")
            .expect("inspect request should be written");

        let mut state = DaemonState {
            health: HealthStatus::Healthy,
            requests: RequestRegistry::new(Duration::from_secs(5)),
            running: true,
        };
        state
            .requests
            .begin_at(
                "req-1",
                RequestOwner::new(local_sender(), Some("org.test.App".to_string())),
                Some(ParentWindowContext::X11 { window_id: 42 }),
                Instant::now(),
            )
            .expect("request should be created");
        state
            .requests
            .transition(
                "req-1",
                &RequestOwner::new(local_sender(), Some("org.test.App".to_string())),
                RequestState::AwaitingUser,
            )
            .expect("request should transition");
        handle_connection(server, &mut state).expect("inspect should be handled");

        let mut response_line = String::new();
        let mut reader = BufReader::new(client);
        reader
            .read_line(&mut response_line)
            .expect("response should be readable");
        let response = ControlResponse::parse_line(&response_line).expect("response should parse");
        assert_eq!(
            response,
            ControlResponse::RequestSnapshot {
                id: "req-1".to_string(),
                state: "awaiting_user".to_string(),
                sender: local_sender(),
                app_id: Some("org.test.App".to_string()),
                parent_window: Some("x11:0x2a".to_string()),
            }
        );
    }

    #[test]
    fn inspect_missing_request_maps_to_not_found() {
        let (mut client, server) = UnixStream::pair().expect("pair should be created");
        client
            .write_all(b"inspect id=req-missing\n")
            .expect("inspect request should be written");

        let mut state = DaemonState {
            health: HealthStatus::Healthy,
            requests: RequestRegistry::new(Duration::from_secs(5)),
            running: true,
        };
        handle_connection(server, &mut state).expect("inspect should be handled");

        let mut response_line = String::new();
        let mut reader = BufReader::new(client);
        reader
            .read_line(&mut response_line)
            .expect("response should be readable");
        let response = ControlResponse::parse_line(&response_line).expect("response should parse");
        assert_eq!(
            response,
            ControlResponse::Error {
                code: 2,
                reason: "request_not_found".to_string(),
            }
        );
    }

    #[test]
    fn inspect_invalid_request_id_maps_to_invalid_request() {
        let (mut client, server) = UnixStream::pair().expect("pair should be created");
        client
            .write_all(b"inspect id=req/invalid\n")
            .expect("inspect request should be written");

        let mut state = DaemonState {
            health: HealthStatus::Healthy,
            requests: RequestRegistry::new(Duration::from_secs(5)),
            running: true,
        };
        handle_connection(server, &mut state).expect("inspect should be handled");

        let mut response_line = String::new();
        let mut reader = BufReader::new(client);
        reader
            .read_line(&mut response_line)
            .expect("response should be readable");
        let response = ControlResponse::parse_line(&response_line).expect("response should parse");
        assert_eq!(
            response,
            ControlResponse::Error {
                code: 2,
                reason: "invalid_request".to_string(),
            }
        );
    }

    #[test]
    fn startup_recovery_expires_non_terminal_requests() {
        let path = unique_temp_file();

        let mut persisted = RequestRegistry::new(Duration::from_secs(5));
        persisted
            .begin_at(
                "req-pending",
                RequestOwner::new(":1.2", None),
                None,
                Instant::now(),
            )
            .expect("request should be created");
        persisted
            .begin_at(
                "req-done",
                RequestOwner::new(":1.3", None),
                None,
                Instant::now(),
            )
            .expect("request should be created");
        persisted
            .transition(
                "req-done",
                &RequestOwner::new(":1.3", None),
                RequestState::AwaitingUser,
            )
            .expect("request should transition");
        persisted
            .transition(
                "req-done",
                &RequestOwner::new(":1.3", None),
                RequestState::Fulfilled,
            )
            .expect("request should transition");
        request_store::persist_registry(&path, &persisted).expect("request store should persist");

        let (loaded, recovered) = load_registry_with_recovery(&path, Duration::from_secs(5))
            .expect("registry should load");
        assert_eq!(recovered, vec!["req-pending".to_string()]);
        assert_eq!(loaded.state("req-pending"), Some(RequestState::Expired));
        assert_eq!(loaded.state("req-done"), Some(RequestState::Fulfilled));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn invalid_store_load_uses_empty_registry_and_quarantines_file() {
        let path = unique_temp_file();
        fs::write(&path, "id=req-1\tsender=:1.2\tstate=bogus\n")
            .expect("invalid store should be written");

        let parent = path
            .parent()
            .expect("temp file should have parent")
            .to_path_buf();
        let file_name = path
            .file_name()
            .expect("temp file should have name")
            .to_string_lossy()
            .to_string();

        let (registry, recovered_ids, degraded) =
            load_registry_with_fallback(&path, Duration::from_secs(5));
        assert!(degraded);
        assert!(recovered_ids.is_empty());
        assert_eq!(registry.total_count(), 0);
        assert!(!path.exists());

        let quarantined = fs::read_dir(&parent)
            .expect("parent dir should be readable")
            .filter_map(Result::ok)
            .find(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                name.starts_with(&format!("{file_name}.corrupt-"))
            })
            .map(|entry| entry.path())
            .expect("quarantined store should exist");
        fs::remove_file(quarantined).expect("quarantined store should be removed");
    }
}
