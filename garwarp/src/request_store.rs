#![allow(dead_code)]

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::request::{RequestOwner, RequestRecord, RequestRegistry, RequestState};
use crate::window::parse_optional_parent_window;

pub fn load_registry(path: &Path, timeout: Duration) -> io::Result<RequestRegistry> {
    let mut registry = RequestRegistry::new(timeout);
    if !path.exists() {
        return Ok(registry);
    }

    let body = fs::read_to_string(path)?;
    let now = Instant::now();

    for (index, line) in body.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record = parse_record_line(line).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid request store line {}: {error}", index + 1),
            )
        })?;
        registry.restore_record(record, now).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid request store line {}: {error}", index + 1),
            )
        })?;
    }

    Ok(registry)
}

pub fn persist_registry(path: &Path, registry: &RequestRegistry) -> io::Result<()> {
    let mut output = String::new();
    for record in registry.records() {
        output.push_str(&format_record_line(&record));
        output.push('\n');
    }
    fs::write(path, output)
}

fn format_record_line(record: &RequestRecord) -> String {
    let app_id = record.owner.app_id.as_deref().unwrap_or("-");
    let parent_window = match record.parent_window {
        Some(parent_window) => parent_window.as_str(),
        None => "-".to_string(),
    };

    format!(
        "id={}\tsender={}\tapp_id={}\tparent={}\tstate={}",
        record.id,
        record.owner.sender,
        app_id,
        parent_window,
        record.state.as_str()
    )
}

fn parse_record_line(line: &str) -> Result<RequestRecord, String> {
    let fields = parse_fields(line)?;

    let id = required_field(&fields, "id")?.to_string();
    let sender = required_field(&fields, "sender")?.to_string();
    let app_id = optional_field(&fields, "app_id");
    let parent_window = parse_optional_parent_window(optional_field_ref(&fields, "parent"))
        .map_err(|error| error.to_string())?;
    let state = RequestState::parse(required_field(&fields, "state")?)
        .ok_or_else(|| "invalid request state".to_string())?;

    Ok(RequestRecord {
        id,
        owner: RequestOwner::new(sender, app_id),
        parent_window,
        state,
    })
}

fn parse_fields(line: &str) -> Result<HashMap<&str, &str>, String> {
    let mut fields = HashMap::new();
    for token in line.split('\t') {
        let (key, value) = token
            .split_once('=')
            .ok_or_else(|| format!("invalid token: {token}"))?;
        fields.insert(key, value);
    }
    Ok(fields)
}

fn required_field<'a>(fields: &'a HashMap<&str, &'a str>, key: &str) -> Result<&'a str, String> {
    fields
        .get(key)
        .copied()
        .ok_or_else(|| format!("missing field: {key}"))
}

fn optional_field(fields: &HashMap<&str, &str>, key: &str) -> Option<String> {
    optional_field_ref(fields, key).map(ToOwned::to_owned)
}

fn optional_field_ref<'a>(fields: &'a HashMap<&str, &'a str>, key: &str) -> Option<&'a str> {
    fields
        .get(key)
        .copied()
        .and_then(|value| if value == "-" { None } else { Some(value) })
}

#[cfg(test)]
mod tests {
    use super::{load_registry, persist_registry};
    use crate::request::{RequestOwner, RequestRegistry, RequestState};
    use crate::window::ParentWindowContext;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    fn unique_temp_file() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        std::env::temp_dir().join(format!("garwarp-request-store-{nanos}.state"))
    }

    #[test]
    fn persist_and_load_roundtrip() {
        let path = unique_temp_file();
        let mut registry = RequestRegistry::new(Duration::from_secs(5));
        registry
            .begin_at(
                "req-1",
                RequestOwner::new(":1.2", Some("org.test.App".to_string())),
                Some(ParentWindowContext::X11 { window_id: 42 }),
                Instant::now(),
            )
            .expect("request should be created");
        registry
            .transition(
                "req-1",
                &RequestOwner::new(":1.2", Some("org.test.App".to_string())),
                RequestState::AwaitingUser,
            )
            .expect("request should transition");

        persist_registry(&path, &registry).expect("registry should persist");

        let loaded = load_registry(&path, Duration::from_secs(5)).expect("registry should load");
        assert_eq!(loaded.state("req-1"), Some(RequestState::AwaitingUser));
        assert_eq!(
            loaded.parent_window("req-1"),
            Some(Some(ParentWindowContext::X11 { window_id: 42 }))
        );
        assert_eq!(
            loaded.owner("req-1"),
            Some(RequestOwner::new(":1.2", Some("org.test.App".to_string())))
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn invalid_lines_fail_to_load() {
        let path = unique_temp_file();
        fs::write(&path, "id=req-1\tsender=:1.2\tstate=bogus\n")
            .expect("test file should be written");

        let loaded = load_registry(&path, Duration::from_secs(5));
        assert!(loaded.is_err());

        let _ = fs::remove_file(path);
    }
}
