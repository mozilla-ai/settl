//! Background update checker -- queries GitHub Releases API and caches results.
//!
//! On TUI startup, a background task checks for newer releases. Results are
//! cached to `~/.settl/update_cache.json` and throttled by a configurable
//! interval (default 24 hours). The main menu displays a non-intrusive badge
//! when a newer version is available.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// How often to re-check (in seconds). Default: 24 hours.
const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// HTTP timeout for the GitHub API request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// GitHub API endpoint for the latest release.
const RELEASES_URL: &str = "https://api.github.com/repos/mozilla-ai/settl/releases/latest";

/// Info about an available update.
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
}

/// On-disk cache so we don't hit the API on every launch.
#[derive(Debug, Serialize, Deserialize)]
struct UpdateCache {
    /// Unix timestamp of the last successful check.
    checked_at: u64,
    /// The latest version string from GitHub (without leading 'v').
    latest_version: String,
}

/// Check GitHub for a newer release, using the disk cache when fresh.
///
/// Returns `Some(UpdateInfo)` if a newer version exists, `None` otherwise.
/// Any errors (network, parse, filesystem) are silently swallowed -- this
/// feature must never block or crash the app.
pub async fn check_for_update() -> Option<UpdateInfo> {
    let current = env!("CARGO_PKG_VERSION");
    let cache_path = cache_path();

    // Try the cache first.
    if let Some(cached) = read_cache(&cache_path) {
        let now = now_unix();
        if now.saturating_sub(cached.checked_at) < CHECK_INTERVAL_SECS {
            // Cache is still fresh.
            return if is_newer(&cached.latest_version, current) {
                Some(UpdateInfo {
                    current_version: current.to_string(),
                    latest_version: cached.latest_version,
                })
            } else {
                None
            };
        }
    }

    // Cache is stale or missing -- fetch from GitHub.
    let latest = fetch_latest_version().await?;

    // Write the cache (best-effort).
    let _ = write_cache(&cache_path, &latest);

    if is_newer(&latest, current) {
        Some(UpdateInfo {
            current_version: current.to_string(),
            latest_version: latest,
        })
    } else {
        None
    }
}

/// Fetch the latest release tag from GitHub.
async fn fetch_latest_version() -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(concat!("settl/", env!("CARGO_PKG_VERSION")))
        .build()
        .ok()?;

    let resp = client.get(RELEASES_URL).send().await.ok()?;

    if !resp.status().is_success() {
        log::debug!("Update check: GitHub API returned {}", resp.status());
        return None;
    }

    let body: serde_json::Value = resp.json().await.ok()?;
    let tag = body.get("tag_name")?.as_str()?;

    // Strip leading 'v' if present (e.g. "v0.2.0" -> "0.2.0").
    let version = tag.strip_prefix('v').unwrap_or(tag);
    Some(version.to_string())
}

/// Return `true` if `candidate` is a strictly newer semver than `current`.
fn is_newer(candidate: &str, current: &str) -> bool {
    let parse = |s: &str| -> Option<Vec<u64>> {
        s.split('.').map(|part| part.parse::<u64>().ok()).collect()
    };
    let c = match parse(candidate) {
        Some(v) => v,
        None => return false,
    };
    let cur = match parse(current) {
        Some(v) => v,
        None => return false,
    };

    // Compare component by component (major, minor, patch, ...).
    for (a, b) in c.iter().zip(cur.iter()) {
        if a > b {
            return true;
        }
        if a < b {
            return false;
        }
    }
    // If all shared components are equal, longer version is "newer"
    // (e.g. "1.0.0.1" > "1.0.0").
    c.len() > cur.len()
}

// ── Cache I/O ─────────────────────────────────────────────────────────

fn cache_path() -> PathBuf {
    let home = std::env::var("HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| ".".into());
    PathBuf::from(home).join(".settl").join("update_cache.json")
}

fn read_cache(path: &PathBuf) -> Option<UpdateCache> {
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

fn write_cache(path: &PathBuf, latest_version: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    let cache = UpdateCache {
        checked_at: now_unix(),
        latest_version: latest_version.to_string(),
    };
    let json = serde_json::to_string(&cache).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_version_detected() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("0.1.1", "0.1.0"));
        assert!(is_newer("2.0.0", "1.99.99"));
    }

    #[test]
    fn same_version_not_newer() {
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("1.0.0", "1.0.0"));
    }

    #[test]
    fn older_version_not_newer() {
        assert!(!is_newer("0.1.0", "0.2.0"));
        assert!(!is_newer("0.9.0", "1.0.0"));
    }

    #[test]
    fn malformed_versions_return_false() {
        assert!(!is_newer("abc", "0.1.0"));
        assert!(!is_newer("0.1.0", "abc"));
        assert!(!is_newer("", "0.1.0"));
        assert!(!is_newer("0.1.0", ""));
    }

    #[test]
    fn extra_components_handled() {
        assert!(is_newer("0.1.0.1", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.1.0.1"));
    }

    #[test]
    fn cache_roundtrip() {
        let dir = std::env::temp_dir().join("settl_test_cache");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("update_cache.json");

        write_cache(&path, "1.2.3").unwrap();
        let cached = read_cache(&path).unwrap();
        assert_eq!(cached.latest_version, "1.2.3");
        assert!(cached.checked_at > 0);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
