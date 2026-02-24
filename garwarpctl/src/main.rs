use std::env;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use garwarp_ipc::{
    ControlRequest, ControlResponse, DEFAULT_CONTROL_SOCKET, DEFAULT_RUNTIME_SUBDIR,
    PROTOCOL_VERSION,
};

fn main() {
    let command = parse_command(env::args().nth(1).as_deref());
    if let Err(error) = run(command) {
        eprintln!("garwarpctl error: {error}");
        std::process::exit(1);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Status,
    Stop,
    Version,
    Help,
}

fn parse_command(input: Option<&str>) -> Command {
    match input {
        Some("status") | None => Command::Status,
        Some("stop") => Command::Stop,
        Some("version") | Some("--version") | Some("-V") => Command::Version,
        Some("help") | Some("--help") | Some("-h") => Command::Help,
        Some(_) => Command::Help,
    }
}

fn run(command: Command) -> io::Result<()> {
    match command {
        Command::Status => {
            let response = send_request(ControlRequest::Status)?;
            match response {
                ControlResponse::Status(status) => {
                    println!("protocol={}", status.protocol_version);
                    println!("health={}", status.health.as_str());
                    println!("in_flight={}", status.in_flight_requests);
                    Ok(())
                }
                ControlResponse::Error { reason } => {
                    Err(io::Error::other(format!("daemon error: {reason}")))
                }
                other => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unexpected response: {other:?}"),
                )),
            }
        }
        Command::Stop => {
            let response = send_request(ControlRequest::Stop)?;
            match response {
                ControlResponse::AckStopping => {
                    println!("stopping");
                    Ok(())
                }
                ControlResponse::Error { reason } => {
                    Err(io::Error::other(format!("daemon error: {reason}")))
                }
                other => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unexpected response: {other:?}"),
                )),
            }
        }
        Command::Version => {
            println!("garwarpctl protocol v{PROTOCOL_VERSION}");
            Ok(())
        }
        Command::Help => {
            print_help();
            Ok(())
        }
    }
}

fn send_request(request: ControlRequest) -> io::Result<ControlResponse> {
    let socket_path = control_socket_path();
    let mut stream = UnixStream::connect(&socket_path)?;
    stream.write_all(request.as_line().as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    ControlResponse::parse_line(&line)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn control_socket_path() -> PathBuf {
    runtime_dir().join(DEFAULT_CONTROL_SOCKET)
}

fn runtime_dir() -> PathBuf {
    let base = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join(DEFAULT_RUNTIME_SUBDIR)
}

fn print_help() {
    println!("garwarpctl <command>");
    println!("commands: status (default), stop, version, help");
}

#[cfg(test)]
mod tests {
    use super::{Command, parse_command};

    #[test]
    fn status_is_default_command() {
        assert_eq!(parse_command(None), Command::Status);
    }

    #[test]
    fn help_for_unknown_command() {
        assert_eq!(parse_command(Some("bogus")), Command::Help);
    }
}
