pub mod download;
pub mod process;

pub use download::ensure_llamafile;
pub use process::LlamafileProcess;

/// Status updates sent from the llamafile setup task to the UI.
#[derive(Debug, Clone)]
pub enum LlamafileStatus {
    /// Checking if the llamafile already exists.
    Checking,
    /// Downloading with progress: (bytes_downloaded, total_bytes).
    Downloading { bytes: u64, total: u64 },
    /// Making the file executable and preparing to launch.
    Preparing,
    /// Starting the llamafile process.
    Starting,
    /// Waiting for the server to become ready.
    WaitingForReady,
    /// Ready to accept connections on this port.
    Ready(u16),
    /// An error occurred.
    Error(String),
}

/// Format a byte count for human-readable display.
pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1048576), "1.0 MB");
        assert_eq!(format_bytes(1073741824), "1.00 GB");
    }
}
