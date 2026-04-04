//! System information detection (RAM, etc.) for resource warnings.

/// Detect total system RAM in GB.
///
/// Returns `None` if detection fails (unsupported platform, permission error, etc.).
pub fn total_ram_gb() -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        linux_total_ram_gb()
    }
    #[cfg(target_os = "macos")]
    {
        macos_total_ram_gb()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn linux_total_ram_gb() -> Option<u32> {
    let contents = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in contents.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            let kb_str = rest.trim().strip_suffix("kB")?.trim();
            let kb: u64 = kb_str.parse().ok()?;
            return Some((kb / (1024 * 1024)) as u32);
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn macos_total_ram_gb() -> Option<u32> {
    let output = std::process::Command::new("sysctl")
        .arg("-n")
        .arg("hw.memsize")
        .output()
        .ok()?;
    let bytes_str = String::from_utf8_lossy(&output.stdout);
    let bytes: u64 = bytes_str.trim().parse().ok()?;
    Some((bytes / (1024 * 1024 * 1024)) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_ram_gb_returns_reasonable_value() {
        // On any CI/dev machine this should return Some with > 0 GB.
        if let Some(ram) = total_ram_gb() {
            assert!(ram > 0, "RAM should be > 0 GB, got {ram}");
            assert!(ram < 4096, "RAM should be < 4 TB, got {ram}");
        }
        // On unsupported platforms, None is fine.
    }
}
