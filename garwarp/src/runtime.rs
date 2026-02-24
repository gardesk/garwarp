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
        Self {
            root,
            control_socket,
            lock_file,
        }
    }

    pub fn ensure_runtime_dir(&self) -> io::Result<()> {
        fs::create_dir_all(&self.root)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

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
    }
}
