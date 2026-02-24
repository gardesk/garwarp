use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::thread;
use std::time::{Duration, Instant};

use garwarp_ipc::{
    ControlRequest, ControlResponse, HealthStatus, RequestTransitionTarget, StatusResponse,
};

use crate::config::Config;
use crate::dbus::{self, SessionNameGuard};
use crate::error::{PortalError, map_portal_error, map_request_error};
use crate::lock::SingleInstanceGuard;
use crate::logging;
use crate::request::{RequestOwner, RequestRegistry, RequestState};
use crate::runtime::RuntimePaths;
use crate::window::parse_optional_parent_window;

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

    let mut state = DaemonState {
        health: HealthStatus::Healthy,
        requests: RequestRegistry::new(Duration::from_secs(30)),
        running: true,
    };

    while state.running {
        let expired = state.requests.expire_stale(Instant::now());
        for id in expired {
            logging::warn(&format!("request_expired id={id}"));
        }

        match listener.accept() {
            Ok((stream, _address)) => {
                if let Err(error) = handle_connection(stream, &mut state) {
                    logging::warn(&format!("request_error={error}"));
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
    reader.read_line(&mut line)?;

    let response = match ControlRequest::parse_line(&line) {
        Some(ControlRequest::Status) => ControlResponse::Status(StatusResponse {
            protocol_version: garwarp_ipc::PROTOCOL_VERSION,
            health: state.health,
            in_flight_requests: state.requests.in_flight_count(),
        }),
        Some(ControlRequest::Stop) => {
            state.health = HealthStatus::Stopping;
            state.running = false;
            ControlResponse::AckStopping
        }
        Some(ControlRequest::BeginRequest {
            id,
            sender,
            app_id,
            parent_window,
        }) => {
            let owner = RequestOwner::new(sender, app_id);
            let parsed_parent_window = match parse_optional_parent_window(parent_window.as_deref())
            {
                Ok(parent_window) => parent_window,
                Err(_) => {
                    let mapping = map_portal_error(&PortalError::InvalidParentWindow);
                    return write_response(
                        reader.into_inner(),
                        ControlResponse::Error {
                            reason: mapping.reason.to_string(),
                        },
                    );
                }
            };

            match state
                .requests
                .begin(id.clone(), owner, parsed_parent_window)
            {
                Ok(()) => ControlResponse::AckRequest {
                    id,
                    state: "pending".to_string(),
                },
                Err(error) => {
                    let mapping = map_request_error(&error);
                    ControlResponse::Error {
                        reason: mapping.reason.to_string(),
                    }
                }
            }
        }
        Some(ControlRequest::TransitionRequest {
            id,
            sender,
            app_id,
            target,
        }) => {
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
                        reason: mapping.reason.to_string(),
                    }
                }
            }
        }
        None => {
            let mapping = map_portal_error(&PortalError::InvalidRequestPayload);
            ControlResponse::Error {
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

#[cfg(test)]
mod tests {
    use super::{DaemonState, handle_connection};
    use garwarp_ipc::{ControlResponse, HealthStatus};
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    use crate::request::{RequestOwner, RequestRegistry, RequestState};
    use crate::window::ParentWindowContext;

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
                RequestOwner::new(":1.2", None),
                None,
                Instant::now(),
            )
            .expect("request should be created");
        state
            .requests
            .transition(
                "req-1",
                &RequestOwner::new(":1.2", None),
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
                reason: "invalid_parent_window".to_string(),
            }
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
                RequestOwner::new(":1.2", None),
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
                reason: "ownership_mismatch".to_string(),
            }
        );
        assert_eq!(state.requests.state("req-1"), Some(RequestState::Pending));
    }
}
