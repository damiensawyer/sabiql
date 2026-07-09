use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

pub const CONFIG_FILE_NAME: &str = "connections.toml";
pub const LAST_CONNECTION_FILE_NAME: &str = "last_connection_id.txt";

static WRITE_COUNTER: AtomicU64 = AtomicU64::new(0);
static CONFIG_FILE_LOCK: Mutex<()> = Mutex::new(());

pub fn lock() -> MutexGuard<'static, ()> {
    CONFIG_FILE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn get_config_dir() -> Result<PathBuf, std::io::Error> {
    let config_base = dirs::config_dir().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Could not find config directory",
        )
    })?;
    Ok(config_base.join("sabiql"))
}

pub fn config_file_path(config_dir: &Path) -> PathBuf {
    config_dir.join(CONFIG_FILE_NAME)
}

pub fn write_config_file(config_dir: &Path, content: &str) -> Result<(), std::io::Error> {
    if !config_dir.exists() {
        fs::create_dir_all(config_dir)?;
    }

    let path = config_file_path(config_dir);
    let counter = WRITE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_path = config_dir.join(format!(
        ".connections.toml.{}-{}.tmp",
        std::process::id(),
        counter,
    ));

    if let Err(e) = fs::write(&tmp_path, content) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    if let Err(e) = set_file_permissions(&tmp_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    if let Err(e) = fs::rename(&tmp_path, &path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    Ok(())
}

pub fn render_config_file(content: &str) -> String {
    format!(
        "# sabiql configuration\n# WARNING: Connection passwords are stored in plain text\n\n{content}"
    )
}

/// Get the path to the temp file that stores the last connection ID.
pub fn last_connection_file_path(config_dir: &Path) -> PathBuf {
    config_dir.join(LAST_CONNECTION_FILE_NAME)
}

/// Write the last connection ID to the temp file.
pub fn write_last_connection_id(
    config_dir: &Path,
    connection_id: &str,
) -> Result<(), std::io::Error> {
    fs::write(last_connection_file_path(config_dir), connection_id)
}

/// Read the last connection ID from the temp file. Returns None if file doesn't exist.
pub fn read_last_connection_id(config_dir: &Path) -> Result<Option<String>, std::io::Error> {
    let path = last_connection_file_path(config_dir);
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)?;
    Ok(Some(content.trim().to_string()))
}

#[cfg(unix)]
fn set_file_permissions(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    let perms = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_file_permissions(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}
