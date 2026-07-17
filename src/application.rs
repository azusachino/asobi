//! Application composition for the command layer.
//!
//! This module is the only place that chooses a concrete storage composite for
//! an application run. Command code receives the stable `api::v2` capabilities
//! through [`AsobiRuntime`] and never needs to name a provider or its state
//! file.

use crate::api::{
    ApiResult, BackendCapabilities, ImportReport, MaintenanceStore, Snapshot, SnapshotStore,
};
use crate::storage::Storage;

/// Tighten handoff-file permissions without coupling the application layer to
/// a provider's physical-backup implementation.
#[cfg(unix)]
pub fn restrict_permissions(path: &std::path::Path, mode: u32) -> crate::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(Into::into)
}

#[cfg(not(unix))]
pub fn restrict_permissions(_path: &std::path::Path, _mode: u32) -> crate::Result<()> {
    Ok(())
}

/// Process-wide application composition root.
pub struct AsobiRuntime {
    storage: Storage,
}

impl AsobiRuntime {
    /// Open the configured default provider. Provider selection and state-file
    /// resolution remain entirely inside `storage`.
    pub fn open_default() -> crate::Result<Self> {
        Ok(Self {
            storage: Storage::open_default()?,
        })
    }

    pub fn from_storage(storage: Storage) -> Self {
        Self { storage }
    }

    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    pub fn into_storage(self) -> Storage {
        self.storage
    }

    pub fn capabilities(&self) -> ApiResult<BackendCapabilities> {
        self.storage.capabilities()
    }

    pub fn export_snapshot(&self, scope: &[String], rationale: bool) -> ApiResult<Snapshot> {
        self.storage.export_snapshot(scope, rationale)
    }

    pub fn import_snapshot(&self, snapshot: Snapshot) -> ApiResult<ImportReport> {
        self.storage.import_snapshot(snapshot)
    }
}
