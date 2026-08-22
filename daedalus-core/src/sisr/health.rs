//! Post-update health validation state machine.
//!
//! After a SISR swap installs a new payload, the launcher runs it under
//! supervision (the "health gate") before trusting it, so a broken update can
//! be rolled back atomically. This module persists that decision per payload
//! version so that a crash before the gate runs, a rollback, or a process
//! restart never loses track of where the update stands.
//!
//! Quarantine is the anti-loop guarantee: once a version has failed the gate
//! `max_attempts` times it is marked `Quarantined` on disk, and the launcher
//! refuses to install or run it again. Because the counter is kept across
//! re-installs, a permanently broken payload cannot churn the user's binary
//! forever.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Default startup watch window; an app still running after this is healthy.
pub const DEFAULT_TIMEOUT_MS: u64 = 10_000;

/// Default number of supervised failures before a version is quarantined.
pub const DEFAULT_MAX_ATTEMPTS: u8 = 3;

/// Policy for the post-update health gate (mission spec, prompt 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthCheckPolicy {
    /// How long the launcher watches the new version at first startup.
    pub timeout_ms: u64,
    /// Failures needed before the version is permanently quarantined.
    pub max_attempts: u8,
}

impl Default for HealthCheckPolicy {
    fn default() -> Self {
        Self {
            timeout_ms: DEFAULT_TIMEOUT_MS,
            max_attempts: DEFAULT_MAX_ATTEMPTS,
        }
    }
}

/// Lifecycle of one payload version in the health store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    /// Update applied, not yet health-validated — must run supervised.
    Pending,
    /// The health gate passed; the version is trusted.
    Healthy,
    /// Failed too many times; refuse to install or run it again.
    Quarantined,
}

/// A persisted record for one payload version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthStatus {
    /// Content-address of the payload (`SHA-256(payload ‖ meta)` hex).
    pub version_id: String,
    pub state: HealthState,
    /// Number of supervised failures recorded for this version.
    pub attempts: u32,
    /// Unix timestamp of the last transition (diagnostics).
    pub updated_secs: u64,
}

/// On-disk, per-version health records. One JSON file per version id in a
/// store directory; writes are atomic (temp file + rename) so an interrupted
/// save can never corrupt the previous record.
pub struct HealthStore {
    dir: PathBuf,
}

impl HealthStore {
    /// Creates a store rooted at `dir` (typically `~/.cache/daedalus/health`).
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
        }
    }

    /// The store's root directory (for diagnostics).
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn record_path(&self, version_id: &str) -> PathBuf {
        self.dir.join(format!("{version_id}.json"))
    }

    /// Loads the record for `version_id`; `Ok(None)` when unknown.
    pub fn load(&self, version_id: &str) -> io::Result<Option<HealthStatus>> {
        let bytes = match fs::read(self.record_path(version_id)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        serde_json::from_slice(&bytes).map(Some).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("bad health record: {e}"),
            )
        })
    }

    /// Marks `version_id` as needing a health check. Called right after an
    /// update swap. The failure counter is preserved across re-installs so a
    /// version keeps accumulating toward quarantine. A `Quarantined` version
    /// is never re-armed.
    pub fn begin(&self, version_id: &str) -> io::Result<HealthStatus> {
        let now = unix_secs();
        let status = match self.load(version_id)? {
            Some(mut existing) => {
                if existing.state == HealthState::Quarantined {
                    return Ok(existing);
                }
                existing.state = HealthState::Pending;
                existing.updated_secs = now;
                existing
            }
            None => HealthStatus {
                version_id: version_id.to_string(),
                state: HealthState::Pending,
                attempts: 0,
                updated_secs: now,
            },
        };
        self.save(&status)?;
        Ok(status)
    }

    /// Records a passed health gate. The version is trusted from now on.
    pub fn confirm(&self, version_id: &str) -> io::Result<HealthStatus> {
        let now = unix_secs();
        let mut status = self.load(version_id)?.unwrap_or_else(|| HealthStatus {
            version_id: version_id.to_string(),
            state: HealthState::Healthy,
            attempts: 0,
            updated_secs: now,
        });
        status.state = HealthState::Healthy;
        status.updated_secs = now;
        self.save(&status)?;
        Ok(status)
    }

    /// Records one supervised failure and returns `true` when the version is
    /// now quarantined (`attempts >= max_attempts`). `max_attempts == 0`
    /// never quarantines.
    pub fn record_failure(&self, version_id: &str, max_attempts: u8) -> io::Result<bool> {
        let now = unix_secs();
        let mut status = self.load(version_id)?.unwrap_or_else(|| HealthStatus {
            version_id: version_id.to_string(),
            state: HealthState::Pending,
            attempts: 0,
            updated_secs: now,
        });
        status.attempts = status.attempts.saturating_add(1);
        status.state = if max_attempts > 0 && status.attempts >= u32::from(max_attempts) {
            HealthState::Quarantined
        } else {
            HealthState::Pending
        };
        status.updated_secs = now;
        self.save(&status)?;
        Ok(status.state == HealthState::Quarantined)
    }

    /// Whether `version_id` is quarantined.
    pub fn is_quarantined(&self, version_id: &str) -> io::Result<bool> {
        Ok(self
            .load(version_id)?
            .is_some_and(|s| s.state == HealthState::Quarantined))
    }

    /// Whether any version has been quarantined. Cheap enough to call on
    /// every update; the launcher only pays for the target-hash pre-check
    /// when at least one broken version exists.
    pub fn has_quarantined(&self) -> io::Result<bool> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        };
        for entry in entries {
            let path = entry?.path();
            let Ok(bytes) = fs::read(&path) else {
                continue;
            };
            let Ok(status) = serde_json::from_slice::<HealthStatus>(&bytes) else {
                continue;
            };
            if status.state == HealthState::Quarantined {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn save(&self, status: &HealthStatus) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join(format!(".{}.tmp", status.version_id));
        let json = serde_json::to_vec(status)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(&tmp, &json)?;
        fs::rename(&tmp, self.record_path(&status.version_id))
    }
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, HealthStore) {
        let tmp = tempfile::tempdir().unwrap();
        let store = HealthStore::new(&tmp.path().join("health"));
        (tmp, store)
    }

    #[test]
    fn load_returns_none_for_unknown_version() {
        let (_tmp, store) = store();
        assert_eq!(store.load("abc").unwrap(), None);
    }

    #[test]
    fn begin_creates_pending_record() {
        let (_tmp, store) = store();
        let status = store.begin("v1").unwrap();
        assert_eq!(status.state, HealthState::Pending);
        assert_eq!(status.attempts, 0);
        assert_eq!(store.load("v1").unwrap(), Some(status));
    }

    #[test]
    fn begin_preserves_failure_counter_across_reinstalls() {
        let (_tmp, store) = store();
        store.record_failure("v1", 3).unwrap();
        let status = store.begin("v1").unwrap();
        assert_eq!(status.state, HealthState::Pending);
        assert_eq!(status.attempts, 1);
    }

    #[test]
    fn begin_will_not_rearm_a_quarantined_version() {
        let (_tmp, store) = store();
        store.record_failure("v1", 1).unwrap();
        assert!(store.is_quarantined("v1").unwrap());
        let status = store.begin("v1").unwrap();
        assert_eq!(status.state, HealthState::Quarantined);
    }

    #[test]
    fn confirm_marks_healthy() {
        let (_tmp, store) = store();
        store.begin("v1").unwrap();
        let status = store.confirm("v1").unwrap();
        assert_eq!(status.state, HealthState::Healthy);
        assert_eq!(
            store.load("v1").unwrap().unwrap().state,
            HealthState::Healthy
        );
    }

    #[test]
    fn record_failure_quarantines_at_threshold() {
        let (_tmp, store) = store();
        assert!(!store.record_failure("v1", 3).unwrap());
        assert!(!store.record_failure("v1", 3).unwrap());
        assert!(!store.is_quarantined("v1").unwrap());
        assert!(store.record_failure("v1", 3).unwrap());
        assert!(store.is_quarantined("v1").unwrap());
    }

    #[test]
    fn zero_max_attempts_never_quarantines() {
        let (_tmp, store) = store();
        store.record_failure("v1", 0).unwrap();
        store.record_failure("v1", 0).unwrap();
        assert!(!store.is_quarantined("v1").unwrap());
    }

    #[test]
    fn has_quarantined_scans_the_store() {
        let (_tmp, store) = store();
        assert!(!store.has_quarantined().unwrap());
        store.begin("good").unwrap();
        assert!(!store.has_quarantined().unwrap());
        store.record_failure("bad", 1).unwrap();
        assert!(store.has_quarantined().unwrap());
    }

    #[test]
    fn has_quarantined_is_false_for_missing_dir() {
        let (_tmp, store) = store();
        assert!(!store.has_quarantined().unwrap());
    }
}
