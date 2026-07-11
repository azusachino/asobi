//! Application composition for the command layer.
//!
//! This module is the only place that chooses a concrete storage composite for
//! an application run. Command code receives the stable `api::v1` capabilities
//! through [`AsobiRuntime`] and never needs to name a provider or its state
//! file.

use crate::api::v1::{
    ApiResult, BackendCapabilities, GraphStore, ImportReport, MaintenanceStore, Snapshot,
    SnapshotStore,
};
use crate::model::EntityInput;
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
    pub async fn open_default() -> crate::Result<Self> {
        Ok(Self {
            storage: Storage::open_default().await?,
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

    pub async fn capabilities(&self) -> ApiResult<BackendCapabilities> {
        self.storage.capabilities().await
    }

    pub async fn export_snapshot(&self, scope: &[String], rationale: bool) -> ApiResult<Snapshot> {
        self.storage.export_snapshot(scope, rationale).await
    }

    pub async fn import_snapshot(&self, snapshot: Snapshot) -> ApiResult<ImportReport> {
        self.storage.import_snapshot(snapshot).await
    }
}

impl SnapshotStore for Storage {
    async fn export_snapshot(&self, scope: &[String], rationale: bool) -> ApiResult<Snapshot> {
        let graph = if scope.is_empty() {
            self.read_graph_full().await?
        } else {
            self.read_graph_scoped(scope, rationale).await?
        };
        let capabilities = self.capabilities().await?;
        Ok(Snapshot {
            api_version: crate::api::v1::API_VERSION,
            format_version: crate::api::v1::SNAPSHOT_FORMAT_VERSION,
            source_backend: capabilities.backend,
            source_schema_version: 1,
            graph,
        })
    }

    async fn import_snapshot(&self, snapshot: Snapshot) -> ApiResult<ImportReport> {
        if snapshot.api_version != crate::api::v1::API_VERSION {
            return Err(crate::api::v1::ApiError::Invalid(format!(
                "unsupported snapshot API version {}",
                snapshot.api_version
            )));
        }
        if snapshot.format_version != crate::api::v1::SNAPSHOT_FORMAT_VERSION {
            return Err(crate::api::v1::ApiError::Invalid(format!(
                "unsupported snapshot format version {}",
                snapshot.format_version
            )));
        }

        let mut report = ImportReport::default();
        for entity in snapshot.graph.entities {
            self.create_entities(vec![EntityInput {
                name: entity.name.clone(),
                entity_type: entity.entity_type,
                observations: entity.observations.clone(),
            }])
            .await?;
            report.entities_created += 1;

            if !entity.observations.is_empty() {
                report.observations_added += entity.observations.len();
            }
            for (key, value) in entity.truths {
                self.truth_upsert(&entity.name, &key, &value).await?;
                report.truths_updated += 1;
            }
        }
        if !snapshot.graph.relations.is_empty() {
            report.relations_added = snapshot.graph.relations.len();
            self.create_relations(snapshot.graph.relations).await?;
        }
        Ok(report)
    }
}
