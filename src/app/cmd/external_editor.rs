use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, bail, WrapErr};

/// Resolve the user's preferred text editor.
///
/// Checks `$EDITOR` first, then `$VISUAL`, then falls back to platform defaults:
/// - Unix: `vi`
/// - Windows: `notepad.exe`
fn resolve_editor() -> PathBuf {
    std::env::var("EDITOR")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("VISUAL")
                .ok()
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| {
            if cfg!(windows) {
                PathBuf::from("notepad.exe")
            } else {
                PathBuf::from("vi")
            }
        })
}

/// Build a std::process::Command to launch the editor for the given file.
fn build_sync_command(editor: &Path, file: &Path) -> std::process::Command {
    let editor_str = editor.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut cmd = std::process::Command::new(editor);

    if editor_str.contains("code") {
        cmd.arg("--wait");
    }
    cmd.arg(file);

    cmd
}

/// Launch the external editor and block until it exits.
///
/// This function must be called from a blocking task (e.g. via
/// `tokio::task::spawn_blocking`) since it waits synchronously on the child
/// process. Terminal state (raw mode / alternate screen / input reader) is
/// the caller's responsibility to suspend and resume around this call —
/// this function only owns the temp file and the child process.
pub(crate) fn run_sync(file: &Path, initial_content: &str) -> Result<String> {
    // Write initial content to temp file
    std::fs::write(file, initial_content)
        .wrap_err_with(|| format!("Failed to write temp file: {}", file.display()))?;

    let editor = resolve_editor();
    let mut child = build_sync_command(&editor, file)
        .spawn()
        .wrap_err_with(|| format!("Failed to spawn editor: {}", editor.display()))?;

    // Wait for the editor to exit (blocking)
    let status = child
        .wait()
        .wrap_err("Editor process failed")?;

    if !status.success() {
        let _ = std::fs::remove_file(file);
        bail!("Editor exited with status: {}", status);
    }

    // Read back the modified content
    let content = std::fs::read_to_string(file)
        .wrap_err_with(|| format!("Failed to read temp file: {}", file.display()))?;

    // Clean up the temp file
    let _ = std::fs::remove_file(file);

    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_editor_from_env() {
        let editor = resolve_editor();
        assert!(!editor.as_os_str().is_empty());
    }

    #[test]
    fn build_sync_command_handles_code() {
        let editor = PathBuf::from("code");
        let file = PathBuf::from("/tmp/test.sql");
        let _cmd = build_sync_command(&editor, &file);
    }

    #[test]
    fn build_sync_command_handles_vim() {
        let editor = PathBuf::from("nvim");
        let file = PathBuf::from("/tmp/test.sql");
        let _cmd = build_sync_command(&editor, &file);
    }

    #[test]
    fn build_sync_command_handles_notepad() {
        let editor = PathBuf::from("notepad.exe");
        let file = PathBuf::from("/tmp/test.sql");
        let _cmd = build_sync_command(&editor, &file);
    }

    #[test]
    fn build_sync_command_fallback() {
        let editor = PathBuf::from("some-editor");
        let file = PathBuf::from("/tmp/test.sql");
        let _cmd = build_sync_command(&editor, &file);
    }
}
