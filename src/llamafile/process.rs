use std::path::Path;

use tokio::process::{Child, Command};

/// Owns a running llamafile child process. Kills it on drop.
pub struct LlamafileProcess {
    child: Child,
    pub port: u16,
}

impl LlamafileProcess {
    /// Start a llamafile process on the given port and wait until it is ready.
    ///
    /// Tries direct execution first. If the binary exits immediately with code
    /// 127 (common on macOS due to Gatekeeper quarantine), retries via `sh`
    /// which bypasses the quarantine check since `/bin/sh` is already trusted.
    pub async fn start(llamafile_path: &Path, port: u16) -> Result<Self, String> {
        let port_str = port.to_string();
        let args = [
            "--server",
            "--port",
            &port_str,
            "--host",
            "127.0.0.1",
            "--parallel",
            "4",
        ];

        log::debug!(
            "llamafile::start path={} port={} exists={} os={} arch={}",
            llamafile_path.display(),
            port,
            llamafile_path.exists(),
            std::env::consts::OS,
            std::env::consts::ARCH,
        );

        #[cfg(unix)]
        if let Ok(meta) = std::fs::metadata(llamafile_path) {
            use std::os::unix::fs::MetadataExt;
            log::debug!("  file size={} mode={:o}", meta.len(), meta.mode(),);
        }

        // Try direct execution first.
        log::debug!("  trying direct execution");
        match Self::spawn_and_wait(llamafile_path, &args, port, false).await {
            Ok(process) => {
                log::debug!("  direct execution succeeded");
                Ok(process)
            }
            Err(e) if e.contains("exit code 127") => {
                log::debug!("  direct execution failed: {e}");
                log::debug!("  retrying via sh");
                let result = Self::spawn_and_wait(llamafile_path, &args, port, true).await;
                match &result {
                    Ok(_) => log::debug!("  sh execution succeeded"),
                    Err(e2) => log::debug!("  sh execution failed: {e2}"),
                }
                result
            }
            Err(e) => {
                log::debug!("  direct execution failed: {e}");
                Err(e)
            }
        }
    }

    /// Spawn the llamafile and poll until the server is ready.
    ///
    /// When `via_sh` is true, runs as `sh <path> <args>` to bypass OS
    /// execution restrictions (macOS Gatekeeper quarantine).
    async fn spawn_and_wait(
        llamafile_path: &Path,
        args: &[&str],
        port: u16,
        via_sh: bool,
    ) -> Result<Self, String> {
        let child = if via_sh {
            Command::new("sh")
                .arg(llamafile_path)
                .args(args)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .map_err(|e| format!("Failed to spawn llamafile via sh: {}", e))?
        } else {
            Command::new(llamafile_path)
                .args(args)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .map_err(|e| format!("Failed to spawn llamafile: {}", e))?
        };

        let mut process = Self { child, port };

        let url = format!("http://127.0.0.1:{}/health", port);
        let client = reqwest::Client::new();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);

        loop {
            if tokio::time::Instant::now() > deadline {
                process.kill();
                return Err("Llamafile failed to start within 60 seconds".into());
            }

            if let Ok(Some(status)) = process.child.try_wait() {
                // Read stderr from the crashed process.
                let stderr_output = if let Some(mut stderr) = process.child.stderr.take() {
                    use tokio::io::AsyncReadExt;
                    let mut buf = Vec::new();
                    let _ = stderr.read_to_end(&mut buf).await;
                    String::from_utf8_lossy(&buf).to_string()
                } else {
                    String::new()
                };
                if !stderr_output.is_empty() {
                    log::debug!("  process stderr:\n{stderr_output}");
                }

                let code = status.code();
                let arch = std::env::consts::ARCH;
                let os = std::env::consts::OS;
                log::debug!("  process exited: status={status} code={code:?}");
                if code == Some(127) {
                    return Err(format!(
                        "Llamafile could not execute (exit code 127) on {}/{}.",
                        os, arch,
                    ));
                }
                return Err(format!(
                    "Llamafile exited unexpectedly ({}) on {}/{}. \
                     Path: {}",
                    status,
                    os,
                    arch,
                    llamafile_path.display(),
                ));
            }

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    log::debug!("  server ready on port {port}");
                    return Ok(process);
                }
                _ => {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            }
        }
    }

    /// Try ports 8080..=8089 and start on the first available one.
    pub async fn start_with_port_scan(llamafile_path: &Path) -> Result<Self, String> {
        let mut last_error = None;
        for port in 8080..=8089 {
            let available = is_port_available(port);
            log::debug!("port_scan: port={port} available={available}");
            if available {
                match Self::start(llamafile_path, port).await {
                    Ok(process) => return Ok(process),
                    Err(e) => {
                        last_error = Some(e);
                    }
                }
            }
        }
        match last_error {
            Some(e) => Err(e),
            None => Err("No available port found in range 8080-8089".into()),
        }
    }

    fn kill(&mut self) {
        // kill_on_drop handles cleanup, but we can be explicit.
        let _ = self.child.start_kill();
    }
}

impl Drop for LlamafileProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Check if a TCP port is available by attempting to bind to it.
fn is_port_available(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}
