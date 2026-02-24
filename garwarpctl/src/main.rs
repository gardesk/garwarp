use std::env;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use garwarp_ipc::{
    ControlRequest, ControlResponse, DEFAULT_CONTROL_SOCKET, DEFAULT_RUNTIME_SUBDIR,
    PROTOCOL_VERSION, RequestTransitionTarget,
};

fn main() {
    let args: Vec<String> = env::args().collect();
    let command = match parse_command(&args[1..]) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("garwarpctl error: {error}");
            print_help();
            std::process::exit(1);
        }
    };

    if let Err(error) = run(command) {
        eprintln!("garwarpctl error: {error}");
        std::process::exit(1);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    Status,
    Stop,
    List,
    Inspect {
        id: String,
    },
    Version,
    Help,
    Begin {
        id: String,
        sender: String,
        app_id: Option<String>,
        parent_window: Option<String>,
    },
    Transition {
        id: String,
        sender: String,
        app_id: Option<String>,
        target: RequestTransitionTarget,
    },
}

fn parse_command(args: &[String]) -> Result<Command, String> {
    match args {
        [] => Ok(Command::Status),
        [command] if command == "status" => Ok(Command::Status),
        [command] if command == "stop" => Ok(Command::Stop),
        [command] if command == "list" => Ok(Command::List),
        [command, id] if command == "inspect" => Ok(Command::Inspect { id: id.clone() }),
        [command] if command == "version" || command == "--version" || command == "-V" => {
            Ok(Command::Version)
        }
        [command] if command == "help" || command == "--help" || command == "-h" => {
            Ok(Command::Help)
        }
        [command, id, sender] if command == "begin" => Ok(Command::Begin {
            id: id.clone(),
            sender: sender.clone(),
            app_id: None,
            parent_window: None,
        }),
        [command, id, sender, app_id] if command == "begin" => Ok(Command::Begin {
            id: id.clone(),
            sender: sender.clone(),
            app_id: optional_value(app_id),
            parent_window: None,
        }),
        [command, id, sender, app_id, parent_window] if command == "begin" => Ok(Command::Begin {
            id: id.clone(),
            sender: sender.clone(),
            app_id: optional_value(app_id),
            parent_window: optional_value(parent_window),
        }),
        [command, id, sender, state] if command == "transition" => Ok(Command::Transition {
            id: id.clone(),
            sender: sender.clone(),
            app_id: None,
            target: parse_transition_target(state)?,
        }),
        [command, id, sender, state, app_id] if command == "transition" => {
            Ok(Command::Transition {
                id: id.clone(),
                sender: sender.clone(),
                app_id: optional_value(app_id),
                target: parse_transition_target(state)?,
            })
        }
        [command, id, sender] if command == "await" => Ok(Command::Transition {
            id: id.clone(),
            sender: sender.clone(),
            app_id: None,
            target: RequestTransitionTarget::AwaitingUser,
        }),
        [command, id, sender, app_id] if command == "await" => Ok(Command::Transition {
            id: id.clone(),
            sender: sender.clone(),
            app_id: optional_value(app_id),
            target: RequestTransitionTarget::AwaitingUser,
        }),
        [command, id, sender] if command == "fulfill" => Ok(Command::Transition {
            id: id.clone(),
            sender: sender.clone(),
            app_id: None,
            target: RequestTransitionTarget::Fulfilled,
        }),
        [command, id, sender, app_id] if command == "fulfill" => Ok(Command::Transition {
            id: id.clone(),
            sender: sender.clone(),
            app_id: optional_value(app_id),
            target: RequestTransitionTarget::Fulfilled,
        }),
        [command, id, sender] if command == "cancel" => Ok(Command::Transition {
            id: id.clone(),
            sender: sender.clone(),
            app_id: None,
            target: RequestTransitionTarget::Cancelled,
        }),
        [command, id, sender, app_id] if command == "cancel" => Ok(Command::Transition {
            id: id.clone(),
            sender: sender.clone(),
            app_id: optional_value(app_id),
            target: RequestTransitionTarget::Cancelled,
        }),
        [command, id, sender] if command == "fail" => Ok(Command::Transition {
            id: id.clone(),
            sender: sender.clone(),
            app_id: None,
            target: RequestTransitionTarget::Failed,
        }),
        [command, id, sender, app_id] if command == "fail" => Ok(Command::Transition {
            id: id.clone(),
            sender: sender.clone(),
            app_id: optional_value(app_id),
            target: RequestTransitionTarget::Failed,
        }),
        _ => Err("unknown command or invalid arguments".to_string()),
    }
}

fn parse_transition_target(value: &str) -> Result<RequestTransitionTarget, String> {
    match value {
        "awaiting_user" => Ok(RequestTransitionTarget::AwaitingUser),
        "fulfilled" => Ok(RequestTransitionTarget::Fulfilled),
        "cancelled" => Ok(RequestTransitionTarget::Cancelled),
        "failed" => Ok(RequestTransitionTarget::Failed),
        _ => Err(format!("unsupported transition state: {value}")),
    }
}

fn optional_value(value: &str) -> Option<String> {
    if value == "-" || value.is_empty() {
        None
    } else {
        Some(value.to_string())
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
                    println!("total={}", status.total_requests);
                    println!("terminal={}", status.terminal_requests);
                    Ok(())
                }
                ControlResponse::Error { code, reason } => Err(io::Error::other(format!(
                    "daemon error: code={code} reason={reason}"
                ))),
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
                ControlResponse::Error { code, reason } => Err(io::Error::other(format!(
                    "daemon error: code={code} reason={reason}"
                ))),
                other => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unexpected response: {other:?}"),
                )),
            }
        }
        Command::List => {
            let response = send_request(ControlRequest::ListRequests)?;
            match response {
                ControlResponse::RequestList { ids } => {
                    if ids.is_empty() {
                        println!("ids=-");
                    } else {
                        for id in ids {
                            println!("id={id}");
                        }
                    }
                    Ok(())
                }
                ControlResponse::Error { code, reason } => Err(io::Error::other(format!(
                    "daemon error: code={code} reason={reason}"
                ))),
                other => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unexpected response: {other:?}"),
                )),
            }
        }
        Command::Inspect { id } => {
            let response = send_request(ControlRequest::InspectRequest { id })?;
            match response {
                ControlResponse::RequestSnapshot {
                    id,
                    state,
                    sender,
                    app_id,
                    parent_window,
                } => {
                    println!("id={id}");
                    println!("state={state}");
                    println!("sender={sender}");
                    println!("app_id={}", app_id.unwrap_or_else(|| "-".to_string()));
                    println!(
                        "parent_window={}",
                        parent_window.unwrap_or_else(|| "-".to_string())
                    );
                    Ok(())
                }
                ControlResponse::Error { code, reason } => Err(io::Error::other(format!(
                    "daemon error: code={code} reason={reason}"
                ))),
                other => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unexpected response: {other:?}"),
                )),
            }
        }
        Command::Begin {
            id,
            sender,
            app_id,
            parent_window,
        } => {
            let response = send_request(ControlRequest::BeginRequest {
                id,
                sender,
                app_id,
                parent_window,
            })?;
            match response {
                ControlResponse::AckRequest { id, state } => {
                    println!("id={id}");
                    println!("state={state}");
                    Ok(())
                }
                ControlResponse::Error { code, reason } => Err(io::Error::other(format!(
                    "daemon error: code={code} reason={reason}"
                ))),
                other => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unexpected response: {other:?}"),
                )),
            }
        }
        Command::Transition {
            id,
            sender,
            app_id,
            target,
        } => {
            let response = send_request(ControlRequest::TransitionRequest {
                id,
                sender,
                app_id,
                target,
            })?;
            match response {
                ControlResponse::AckRequest { id, state } => {
                    println!("id={id}");
                    println!("state={state}");
                    Ok(())
                }
                ControlResponse::Error { code, reason } => Err(io::Error::other(format!(
                    "daemon error: code={code} reason={reason}"
                ))),
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
    let line = request.as_line();
    stream.write_all(line.as_bytes())?;
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
    println!("commands:");
    println!("  status (default)");
    println!("  stop");
    println!("  list");
    println!("  inspect <id>");
    println!("  begin <id> <sender> [app_id|-] [parent_window|-]");
    println!("  transition <id> <sender> <awaiting_user|fulfilled|cancelled|failed> [app_id|-]");
    println!("  await|fulfill|cancel|fail <id> <sender> [app_id|-]");
    println!("  version");
    println!("  help");
}

#[cfg(test)]
mod tests {
    use super::{Command, optional_value, parse_command, parse_transition_target};
    use garwarp_ipc::RequestTransitionTarget;

    #[test]
    fn status_is_default_command() {
        assert_eq!(
            parse_command(&[]).expect("status should be default"),
            Command::Status
        );
    }

    #[test]
    fn parse_begin_command_with_parent_window() {
        let args = vec![
            "begin".to_string(),
            "req-1".to_string(),
            ":1.2".to_string(),
            "org.test.App".to_string(),
            "x11:0x2a".to_string(),
        ];
        let command = parse_command(&args).expect("begin command should parse");
        assert_eq!(
            command,
            Command::Begin {
                id: "req-1".to_string(),
                sender: ":1.2".to_string(),
                app_id: Some("org.test.App".to_string()),
                parent_window: Some("x11:0x2a".to_string()),
            }
        );
    }

    #[test]
    fn parse_transition_command() {
        let args = vec![
            "transition".to_string(),
            "req-1".to_string(),
            ":1.2".to_string(),
            "cancelled".to_string(),
        ];
        let command = parse_command(&args).expect("transition command should parse");
        assert_eq!(
            command,
            Command::Transition {
                id: "req-1".to_string(),
                sender: ":1.2".to_string(),
                app_id: None,
                target: RequestTransitionTarget::Cancelled,
            }
        );
    }

    #[test]
    fn parse_inspect_command() {
        let args = vec!["inspect".to_string(), "req-1".to_string()];
        let command = parse_command(&args).expect("inspect command should parse");
        assert_eq!(
            command,
            Command::Inspect {
                id: "req-1".to_string()
            }
        );
    }

    #[test]
    fn parse_list_command() {
        let args = vec!["list".to_string()];
        let command = parse_command(&args).expect("list command should parse");
        assert_eq!(command, Command::List);
    }

    #[test]
    fn parse_transition_target_rejects_unknown_state() {
        let parsed = parse_transition_target("bogus");
        assert!(parsed.is_err());
    }

    #[test]
    fn optional_value_uses_dash_as_none() {
        assert_eq!(optional_value("-"), None);
        assert_eq!(
            optional_value("org.test.App"),
            Some("org.test.App".to_string())
        );
    }
}
