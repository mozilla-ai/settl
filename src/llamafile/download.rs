use std::path::PathBuf;

use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use super::LlamafileStatus;

const LLAMAFILE_NAME: &str = "Bonsai-1.7B.llamafile";
const LLAMAFILE_URL: &str =
    "https://huggingface.co/mozilla-ai/llamafile_0.10.0/resolve/main/Bonsai-1.7B.llamafile?download=true";

/// Minimum file size to consider a cached llamafile valid (100 MB).
/// The actual Bonsai-1.7B file is ~1.7 GB.
const MIN_VALID_SIZE: u64 = 100 * 1024 * 1024;

/// Return the directory where llamafiles are stored: `~/.settl/llamafiles/`.
pub fn llamafile_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| ".".into());
    PathBuf::from(home).join(".settl").join("llamafiles")
}

/// Return the expected path to the Bonsai llamafile.
pub fn llamafile_path() -> PathBuf {
    llamafile_dir().join(LLAMAFILE_NAME)
}

/// Ensure the llamafile exists on disk. Downloads it if missing or corrupted.
///
/// Sends progress updates through `status_tx`. The file is downloaded to a
/// `.tmp` file first and atomically renamed on completion.
pub async fn ensure_llamafile(
    status_tx: mpsc::UnboundedSender<LlamafileStatus>,
) -> Result<PathBuf, String> {
    let path = llamafile_path();

    let _ = status_tx.send(LlamafileStatus::Checking);

    // Check for existing file, but validate its size.
    if path.exists() {
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if size >= MIN_VALID_SIZE {
            log::debug!("llamafile exists and looks valid: size={size}");
            return Ok(path);
        }
        log::debug!("llamafile exists but too small ({size} bytes), re-downloading");
        let _ = std::fs::remove_file(&path);
    }

    // Create parent directory.
    let dir = llamafile_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create directory {}: {}", dir.display(), e))?;

    let tmp_path = dir.join(format!("{}.tmp", LLAMAFILE_NAME));

    let _ = status_tx.send(LlamafileStatus::Downloading { bytes: 0, total: 0 });

    let response = reqwest::get(LLAMAFILE_URL)
        .await
        .map_err(|e| format!("Download failed: {}", e))?;

    let status = response.status();
    log::debug!("download response: status={status} url={LLAMAFILE_URL}");

    if !status.is_success() {
        return Err(format!(
            "Download failed: server returned {} for {}",
            status, LLAMAFILE_URL,
        ));
    }

    let total = response.content_length().unwrap_or(0);
    log::debug!("download content-length: {total}");
    let mut bytes_downloaded: u64 = 0;

    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .map_err(|e| format!("Failed to create temp file: {}", e))?;

    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Download error: {}", e))?
    {
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Write error: {}", e))?;
        bytes_downloaded += chunk.len() as u64;
        let _ = status_tx.send(LlamafileStatus::Downloading {
            bytes: bytes_downloaded,
            total,
        });
    }

    file.flush()
        .await
        .map_err(|e| format!("Flush error: {}", e))?;
    drop(file);

    // Validate downloaded size.
    let actual_size = std::fs::metadata(&tmp_path).map(|m| m.len()).unwrap_or(0);
    log::debug!("download complete: {actual_size} bytes written");
    if actual_size < MIN_VALID_SIZE {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(format!(
            "Download appears incomplete ({} bytes). \
             Expected at least {} bytes. Please try again.",
            actual_size, MIN_VALID_SIZE,
        ));
    }

    let _ = status_tx.send(LlamafileStatus::Preparing);

    // Make executable.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&tmp_path)
            .map_err(|e| format!("Failed to read metadata: {}", e))?
            .permissions();
        perms.set_mode(perms.mode() | 0o755);
        std::fs::set_permissions(&tmp_path, perms)
            .map_err(|e| format!("Failed to set permissions: {}", e))?;
    }

    // On macOS, remove the quarantine attribute so Gatekeeper doesn't block execution.
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("xattr")
            .arg("-d")
            .arg("com.apple.quarantine")
            .arg(&tmp_path)
            .output();
    }

    // Atomic rename.
    std::fs::rename(&tmp_path, &path).map_err(|e| format!("Failed to rename temp file: {}", e))?;

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llamafile_dir_uses_home() {
        std::env::set_var("HOME", "/tmp/test_home");
        let dir = llamafile_dir();
        assert_eq!(dir, PathBuf::from("/tmp/test_home/.settl/llamafiles"));
    }

    #[test]
    fn llamafile_path_includes_filename() {
        std::env::set_var("HOME", "/tmp/test_home");
        let path = llamafile_path();
        assert!(path.ends_with("Bonsai-1.7B.llamafile"));
    }
}
