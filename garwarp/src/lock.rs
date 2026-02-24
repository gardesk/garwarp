use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub struct SingleInstanceGuard {
    path: PathBuf,
}

impl SingleInstanceGuard {
    pub fn acquire(path: &Path) -> io::Result<Self> {
        match try_create_lock(path) {
            Ok(file) => {
                write_pid(file)?;
                Ok(Self {
                    path: path.to_path_buf(),
                })
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if is_stale(path)? {
                    fs::remove_file(path)?;
                    let file = try_create_lock(path)?;
                    write_pid(file)?;
                    return Ok(Self {
                        path: path.to_path_buf(),
                    });
                }
                Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "garwarp daemon is already running",
                ))
            }
            Err(error) => Err(error),
        }
    }
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn try_create_lock(path: &Path) -> io::Result<File> {
    OpenOptions::new().create_new(true).write(true).open(path)
}

fn write_pid(mut file: File) -> io::Result<()> {
    write!(file, "{}", std::process::id())
}

fn is_stale(path: &Path) -> io::Result<bool> {
    let pid_raw = fs::read_to_string(path)?;
    let pid = match pid_raw.trim().parse::<u32>() {
        Ok(pid) => pid,
        Err(_) => return Ok(true),
    };
    Ok(!process_exists(pid))
}

fn process_exists(pid: u32) -> bool {
    PathBuf::from("/proc").join(pid.to_string()).exists()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::SingleInstanceGuard;

    fn unique_temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let dir = std::env::temp_dir().join(format!("garwarp-test-{nanos}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn acquires_stale_lock_file() {
        let dir = unique_temp_dir();
        let lock_path = dir.join("garwarp.lock");
        fs::write(&lock_path, "999999").expect("stale pid should be written");

        let _guard = SingleInstanceGuard::acquire(&lock_path)
            .expect("stale lock should be replaced by current process lock");

        let pid = fs::read_to_string(&lock_path).expect("lock file should exist");
        assert_eq!(pid.trim(), std::process::id().to_string());

        let _ = fs::remove_dir_all(dir);
    }
}
