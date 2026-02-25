use std::collections::HashMap;
use std::env;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use garwarp_ipc::{
    ControlRequest, ControlResponse, DEFAULT_CONTROL_SOCKET, DEFAULT_RUNTIME_SUBDIR,
    PROTOCOL_VERSION, RequestTransitionTarget,
};
use zbus::{
    blocking::Connection,
    zvariant::{OwnedObjectPath, OwnedValue},
};

const BACKEND_DBUS_NAME: &str = "org.freedesktop.impl.portal.desktop.garwarp";
const BACKEND_OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";
const PORTAL_RESPONSE_FAILED: u32 = 2;

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
    PortalSmoke,
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
        [command] if command == "portal-smoke" => Ok(Command::PortalSmoke),
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
        Command::PortalSmoke => run_portal_smoke(),
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

fn run_portal_smoke() -> io::Result<()> {
    let connection = Connection::session()
        .map_err(|error| io::Error::other(format!("failed to connect to session bus: {error}")))?;
    let sender = connection
        .unique_name()
        .ok_or_else(|| io::Error::other("connection missing unique bus name"))?
        .as_str()
        .to_string();
    let sender_segment = sender_to_handle_segment(&sender)?;

    let screenshot_handle = request_handle(&sender_segment, "smoke_screenshot")?;
    let screenshot_response = call_portal_screenshot(&connection, screenshot_handle)?;
    println!("screenshot_response={screenshot_response}");

    let open_file_handle = request_handle(&sender_segment, "smoke_open_file")?;
    let open_file_response = call_portal_open_file(&connection, open_file_handle)?;
    println!("open_file_response={open_file_response}");

    let choose_handle = request_handle(&sender_segment, "smoke_choose_app")?;
    let choose_response = call_portal_choose_application(&connection, choose_handle.clone())?;
    println!("choose_application_response={choose_response}");

    call_portal_update_choices_expect_invalid_transition(&connection, choose_handle)?;
    println!("update_choices=invalid_transition");
    println!("portal_smoke=ok");
    Ok(())
}

fn sender_to_handle_segment(sender: &str) -> io::Result<String> {
    let sender = sender.strip_prefix(':').ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid sender unique name: {sender}"),
        )
    })?;
    if sender.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid sender unique name: empty".to_string(),
        ));
    }

    let mut segment = String::with_capacity(sender.len());
    for ch in sender.chars() {
        if ch.is_ascii_alphanumeric() {
            segment.push(ch);
            continue;
        }
        if matches!(ch, '.' | '_' | '-') {
            segment.push('_');
            continue;
        }
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported sender character: {ch}"),
        ));
    }

    Ok(segment)
}

fn request_handle(sender_segment: &str, token: &str) -> io::Result<OwnedObjectPath> {
    let path = format!("{BACKEND_OBJECT_PATH}/request/{sender_segment}/{token}");
    OwnedObjectPath::try_from(path.clone()).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid request-handle path {path}: {error}"),
        )
    })
}

fn empty_options() -> HashMap<String, OwnedValue> {
    HashMap::new()
}

fn call_portal_screenshot(connection: &Connection, handle: OwnedObjectPath) -> io::Result<u32> {
    let message = connection
        .call_method(
            Some(BACKEND_DBUS_NAME),
            BACKEND_OBJECT_PATH,
            Some("org.freedesktop.impl.portal.Screenshot"),
            "Screenshot",
            &(handle, "org.test.App", "", empty_options()),
        )
        .map_err(|error| io::Error::other(format!("screenshot call failed: {error}")))?;
    expect_failed_response(message)
}

fn call_portal_open_file(connection: &Connection, handle: OwnedObjectPath) -> io::Result<u32> {
    let message = connection
        .call_method(
            Some(BACKEND_DBUS_NAME),
            BACKEND_OBJECT_PATH,
            Some("org.freedesktop.impl.portal.FileChooser"),
            "OpenFile",
            &(handle, "org.test.App", "", "Open", empty_options()),
        )
        .map_err(|error| io::Error::other(format!("open file call failed: {error}")))?;
    expect_failed_response(message)
}

fn call_portal_choose_application(
    connection: &Connection,
    handle: OwnedObjectPath,
) -> io::Result<u32> {
    let choices = vec!["org.test.Viewer".to_string()];
    let message = connection
        .call_method(
            Some(BACKEND_DBUS_NAME),
            BACKEND_OBJECT_PATH,
            Some("org.freedesktop.impl.portal.AppChooser"),
            "ChooseApplication",
            &(handle, "org.test.App", "", choices, empty_options()),
        )
        .map_err(|error| io::Error::other(format!("choose application call failed: {error}")))?;
    expect_failed_response(message)
}

fn call_portal_update_choices_expect_invalid_transition(
    connection: &Connection,
    handle: OwnedObjectPath,
) -> io::Result<()> {
    let choices = vec!["org.test.Viewer".to_string()];
    match connection.call_method(
        Some(BACKEND_DBUS_NAME),
        BACKEND_OBJECT_PATH,
        Some("org.freedesktop.impl.portal.AppChooser"),
        "UpdateChoices",
        &(handle, choices),
    ) {
        Ok(_) => Err(io::Error::other(
            "expected UpdateChoices to fail with invalid_transition".to_string(),
        )),
        Err(error) => {
            let text = error.to_string();
            if !text.contains("invalid_transition") {
                return Err(io::Error::other(format!(
                    "unexpected UpdateChoices error: {text}"
                )));
            }
            Ok(())
        }
    }
}

fn expect_failed_response(message: zbus::message::Message) -> io::Result<u32> {
    let (response, results): (u32, HashMap<String, OwnedValue>) = message
        .body()
        .deserialize()
        .map_err(|error| io::Error::other(format!("failed to decode method reply: {error}")))?;
    if response != PORTAL_RESPONSE_FAILED {
        return Err(io::Error::other(format!(
            "unexpected portal response code: {response}"
        )));
    }
    if !results.is_empty() {
        return Err(io::Error::other(
            "expected empty results map for placeholder implementation".to_string(),
        ));
    }
    Ok(response)
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
    println!("  portal-smoke");
    println!("  inspect <id>");
    println!("  begin <id> <sender> [app_id|-] [parent_window|-]");
    println!("  transition <id> <sender> <awaiting_user|fulfilled|cancelled|failed> [app_id|-]");
    println!("  await|fulfill|cancel|fail <id> <sender> [app_id|-]");
    println!("  version");
    println!("  help");
}

#[cfg(test)]
mod tests {
    use super::{
        Command, optional_value, parse_command, parse_transition_target, sender_to_handle_segment,
    };
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
    fn parse_portal_smoke_command() {
        let args = vec!["portal-smoke".to_string()];
        let command = parse_command(&args).expect("portal-smoke command should parse");
        assert_eq!(command, Command::PortalSmoke);
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

    #[test]
    fn sender_segment_is_derived_from_unique_name() {
        let segment = sender_to_handle_segment(":1.42").expect("segment should parse");
        assert_eq!(segment, "1_42");
    }
}
