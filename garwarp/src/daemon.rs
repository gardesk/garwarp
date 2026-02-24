use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::thread;
use std::time::Duration;

use garwarp_ipc::{ControlRequest, ControlResponse, HealthStatus, StatusResponse};

use crate::config::Config;
use crate::dbus::{self, SessionNameGuard};
use crate::lock::SingleInstanceGuard;
use crate::logging;
use crate::request::RequestRegistry;
use crate::runtime::RuntimePaths;

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
        None => ControlResponse::Error {
            reason: "invalid_request".to_string(),
        },
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

#[cfg(test)]
mod tests {
    use super::{DaemonState, handle_connection};
    use garwarp_ipc::{ControlResponse, HealthStatus};
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    use crate::request::{RequestOwner, RequestRegistry, RequestState};

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
            .begin_at("req-1", RequestOwner::new(":1.2", None), Instant::now())
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
}
