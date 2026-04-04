use std::path::PathBuf;

use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use super::LlamafileStatus;

/// Which Bonsai model to use for local AI.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LlamafileModel {
    /// Bonsai-1.7B: fast, small download (~267 MB), limited reasoning output.
    #[default]
    Bonsai1B,
    /// Bonsai-8B: slower, larger download (~4.9 GB), produces text reasoning blocks.
    Bonsai8B,
}

impl LlamafileModel {
    pub fn filename(self) -> &'static str {
        match self {
            Self::Bonsai1B => "Bonsai-1.7B.llamafile",
            Self::Bonsai8B => "Bonsai-8B.llamafile",
        }
    }

    pub fn url(self) -> &'static str {
        match self {
            Self::Bonsai1B => "https://huggingface.co/mozilla-ai/llamafile_0.10.0/resolve/main/Bonsai-1.7B.llamafile?download=true",
            Self::Bonsai8B => "https://huggingface.co/mozilla-ai/llamafile_0.10.0/resolve/main/Bonsai-8B.llamafile?download=true",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Bonsai1B => "1.7B (fast)",
            Self::Bonsai8B => "8B (smart)",
        }
    }
}

/// Minimum file size to consider a cached llamafile valid (100 MB).
const MIN_VALID_SIZE: u64 = 100 * 1024 * 1024;

/// Return the directory where llamafiles are stored: `~/.settl/llamafiles/`.
pub fn llamafile_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| ".".into());
    PathBuf::from(home).join(".settl").join("llamafiles")
}

/// Return the expected path to a llamafile model.
pub fn llamafile_path(model: LlamafileModel) -> PathBuf {
    llamafile_dir().join(model.filename())
}

/// Ensure the llamafile exists on disk. Downloads it if missing or corrupted.
///
/// Sends progress updates through `status_tx`. The file is downloaded to a
/// `.tmp` file first and atomically renamed on completion.
pub async fn ensure_llamafile(
    model: LlamafileModel,
    status_tx: mpsc::UnboundedSender<LlamafileStatus>,
) -> Result<PathBuf, String> {
    let path = llamafile_path(model);

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

    let tmp_path = dir.join(format!("{}.tmp", model.filename()));

    let _ = status_tx.send(LlamafileStatus::Downloading { bytes: 0, total: 0 });

    let response = reqwest::get(model.url())
        .await
        .map_err(|e| format!("Download failed: {}", e))?;

    let status = response.status();
    let url = model.url();
    log::debug!("download response: status={status} url={url}");

    if !status.is_success() {
        return Err(format!(
            "Download failed: server returned {} for {}",
            status,
            model.url(),
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
        let path = llamafile_path(LlamafileModel::Bonsai1B);
        assert!(path.ends_with("Bonsai-1.7B.llamafile"));
    }

    #[test]
    fn llamafile_path_8b() {
        std::env::set_var("HOME", "/tmp/test_home");
        let path = llamafile_path(LlamafileModel::Bonsai8B);
        assert!(path.ends_with("Bonsai-8B.llamafile"));
    }
}
