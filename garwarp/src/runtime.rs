use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

use garwarp_ipc::{DEFAULT_CONTROL_SOCKET, DEFAULT_RUNTIME_SUBDIR};

#[derive(Debug, Clone)]
pub struct RuntimePaths {
    pub root: PathBuf,
    pub control_socket: PathBuf,
    pub lock_file: PathBuf,
    pub request_store: PathBuf,
}

impl RuntimePaths {
    #[must_use]
    pub fn from_env() -> Self {
        let base = env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(env::temp_dir);
        Self::from_base(base)
    }

    #[must_use]
    pub fn from_base(base: PathBuf) -> Self {
        let root = base.join(DEFAULT_RUNTIME_SUBDIR);
        let control_socket = root.join(DEFAULT_CONTROL_SOCKET);
        let lock_file = root.join("garwarp.lock");
        let request_store = root.join("requests.state");
        Self {
            root,
            control_socket,
            lock_file,
            request_store,
        }
    }

    pub fn ensure_runtime_dir(&self) -> io::Result<()> {
        fs::create_dir_all(&self.root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::RuntimePaths;

    #[test]
    fn from_base_builds_expected_paths() {
        let paths = RuntimePaths::from_base(PathBuf::from("/tmp/runtime"));
        assert_eq!(paths.root, PathBuf::from("/tmp/runtime/garwarp"));
        assert_eq!(
            paths.control_socket,
            PathBuf::from("/tmp/runtime/garwarp/control.sock")
        );
        assert_eq!(
            paths.lock_file,
            PathBuf::from("/tmp/runtime/garwarp/garwarp.lock")
        );
        assert_eq!(
            paths.request_store,
            PathBuf::from("/tmp/runtime/garwarp/requests.state")
        );
    }

    #[cfg(unix)]
    #[test]
    fn ensure_runtime_dir_sets_private_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let base = std::env::temp_dir().join(format!("garwarp-runtime-test-{nanos}"));
        let paths = RuntimePaths::from_base(base.clone());
        paths
            .ensure_runtime_dir()
            .expect("runtime dir should be created");

        let mode = fs::metadata(&paths.root)
            .expect("runtime metadata should be readable")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700);

        fs::remove_dir_all(base).expect("temp runtime dir should be removed");
    }
}
