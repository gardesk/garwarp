mod config;
mod daemon;
mod dbus;
mod error;
mod lock;
mod logging;
mod request;
mod request_store;
mod runtime;
mod window;

use std::env;

use garwarp_ipc::PROTOCOL_VERSION;

fn main() {
    let command = parse_command(env::args().nth(1).as_deref());
    let result = match command {
        Command::Daemon => daemon::run(),
        Command::Version => {
            println!("garwarp protocol v{PROTOCOL_VERSION}");
            Ok(())
        }
        Command::Help => {
            print_help();
            Ok(())
        }
    };

    if let Err(error) = result {
        eprintln!("garwarp error: {error}");
        std::process::exit(1);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Daemon,
    Version,
    Help,
}

fn parse_command(input: Option<&str>) -> Command {
    match input {
        Some("daemon") | None => Command::Daemon,
        Some("version") | Some("--version") | Some("-V") => Command::Version,
        Some("help") | Some("--help") | Some("-h") => Command::Help,
        Some(_) => Command::Help,
    }
}

fn print_help() {
    println!("garwarp <command>");
    println!("commands: daemon (default), version, help");
}

#[cfg(test)]
mod tests {
    use super::{Command, parse_command};

    #[test]
    fn daemon_is_default_command() {
        assert_eq!(parse_command(None), Command::Daemon);
    }

    #[test]
    fn help_for_unknown_command() {
        assert_eq!(parse_command(Some("bogus")), Command::Help);
    }
}
