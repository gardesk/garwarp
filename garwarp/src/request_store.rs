#![allow(dead_code)]

use std::collections::HashMap;
use std::fs;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
    atomic_write(path, output.as_bytes())
}

fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let temp_path = unique_temp_path(path);
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)?;
    #[cfg(unix)]
    {
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    io::Write::write_all(&mut file, data)?;
    file.sync_all()?;
    drop(file);

    fs::rename(&temp_path, path)?;
    Ok(())
}

fn unique_temp_path(path: &Path) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state");
    parent.join(format!(".{file_name}.tmp-{}-{nanos}", std::process::id()))
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
        if !matches!(key, "id" | "sender" | "app_id" | "parent" | "state") {
            return Err(format!("unknown field: {key}"));
        }
        if fields.insert(key, value).is_some() {
            return Err(format!("duplicate field: {key}"));
        }
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

    #[test]
    fn duplicate_fields_fail_to_load() {
        let path = unique_temp_file();
        fs::write(&path, "id=req-1\tid=req-2\tsender=:1.2\tstate=pending\n")
            .expect("test file should be written");

        let loaded = load_registry(&path, Duration::from_secs(5));
        assert!(loaded.is_err());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn unknown_fields_fail_to_load() {
        let path = unique_temp_file();
        fs::write(
            &path,
            "id=req-1\tsender=:1.2\tstate=pending\tunexpected=1\n",
        )
        .expect("test file should be written");

        let loaded = load_registry(&path, Duration::from_secs(5));
        assert!(loaded.is_err());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn persist_overwrites_previous_contents() {
        let path = unique_temp_file();
        let mut first = RequestRegistry::new(Duration::from_secs(5));
        first
            .begin_at(
                "req-first",
                RequestOwner::new(":1.2", None),
                None,
                Instant::now(),
            )
            .expect("request should be created");
        persist_registry(&path, &first).expect("first persist should succeed");

        let mut second = RequestRegistry::new(Duration::from_secs(5));
        second
            .begin_at(
                "req-second",
                RequestOwner::new(":1.3", Some("org.test.App".to_string())),
                None,
                Instant::now(),
            )
            .expect("request should be created");
        persist_registry(&path, &second).expect("second persist should succeed");

        let loaded = load_registry(&path, Duration::from_secs(5)).expect("registry should load");
        assert_eq!(loaded.state("req-first"), None);
        assert_eq!(loaded.state("req-second"), Some(RequestState::Pending));

        let _ = fs::remove_file(path);
    }
}
