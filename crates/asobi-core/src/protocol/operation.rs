//! The operation names, one per `v3` trait method.
//!
//! One call is `POST /v3/graphs/<graph>/<operation>` (ADR 0005): the final
//! path segment names the operation, in `<trait>.<method>` camelCase exactly
//! as the ADR's table lists them. "Method" is reserved for the HTTP verb.
//! This enum is the single parser for that segment, and the schema publisher
//! walks [`Operation::all`] so a new operation cannot be added without its
//! schema row.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    GraphCreateEntities,
    GraphAddObservations,
    GraphCreateRelations,
    GraphDeleteRelations,
    GraphDeleteEntities,
    GraphDeleteObservations,
    GraphDeleteObservationById,
    GraphUpdateObservationById,
    GraphUpdateObservation,
    GraphTruthUpsert,
    GraphTruthDelete,
    GraphReadGraph,
    GraphReadGraphFull,
    GraphOpenNodes,
    SearchNodes,
    MaintenanceStats,
    MaintenanceStatsPerEntity,
    MaintenancePurge,
    MaintenanceReset,
    MaintenanceCapabilities,
    MaintenanceHealth,
    MaintenanceLocation,
    TasksDispatch,
    TasksClaimNext,
}

impl Operation {
    pub fn parse(name: &str) -> Option<Operation> {
        Some(match name {
            "graph.createEntities" => Operation::GraphCreateEntities,
            "graph.addObservations" => Operation::GraphAddObservations,
            "graph.createRelations" => Operation::GraphCreateRelations,
            "graph.deleteRelations" => Operation::GraphDeleteRelations,
            "graph.deleteEntities" => Operation::GraphDeleteEntities,
            "graph.deleteObservations" => Operation::GraphDeleteObservations,
            "graph.deleteObservationById" => Operation::GraphDeleteObservationById,
            "graph.updateObservationById" => Operation::GraphUpdateObservationById,
            "graph.updateObservation" => Operation::GraphUpdateObservation,
            "graph.truthUpsert" => Operation::GraphTruthUpsert,
            "graph.truthDelete" => Operation::GraphTruthDelete,
            "graph.readGraph" => Operation::GraphReadGraph,
            "graph.readGraphFull" => Operation::GraphReadGraphFull,
            "graph.openNodes" => Operation::GraphOpenNodes,
            "search.nodes" => Operation::SearchNodes,
            "maintenance.stats" => Operation::MaintenanceStats,
            "maintenance.statsPerEntity" => Operation::MaintenanceStatsPerEntity,
            "maintenance.purge" => Operation::MaintenancePurge,
            "maintenance.reset" => Operation::MaintenanceReset,
            "maintenance.capabilities" => Operation::MaintenanceCapabilities,
            "maintenance.health" => Operation::MaintenanceHealth,
            "maintenance.location" => Operation::MaintenanceLocation,
            "tasks.dispatch" => Operation::TasksDispatch,
            "tasks.claimNext" => Operation::TasksClaimNext,
            _ => return None,
        })
    }

    /// The wire name, the inverse of [`Method::parse`].
    pub fn name(&self) -> &'static str {
        match self {
            Operation::GraphCreateEntities => "graph.createEntities",
            Operation::GraphAddObservations => "graph.addObservations",
            Operation::GraphCreateRelations => "graph.createRelations",
            Operation::GraphDeleteRelations => "graph.deleteRelations",
            Operation::GraphDeleteEntities => "graph.deleteEntities",
            Operation::GraphDeleteObservations => "graph.deleteObservations",
            Operation::GraphDeleteObservationById => "graph.deleteObservationById",
            Operation::GraphUpdateObservationById => "graph.updateObservationById",
            Operation::GraphUpdateObservation => "graph.updateObservation",
            Operation::GraphTruthUpsert => "graph.truthUpsert",
            Operation::GraphTruthDelete => "graph.truthDelete",
            Operation::GraphReadGraph => "graph.readGraph",
            Operation::GraphReadGraphFull => "graph.readGraphFull",
            Operation::GraphOpenNodes => "graph.openNodes",
            Operation::SearchNodes => "search.nodes",
            Operation::MaintenanceStats => "maintenance.stats",
            Operation::MaintenanceStatsPerEntity => "maintenance.statsPerEntity",
            Operation::MaintenancePurge => "maintenance.purge",
            Operation::MaintenanceReset => "maintenance.reset",
            Operation::MaintenanceCapabilities => "maintenance.capabilities",
            Operation::MaintenanceHealth => "maintenance.health",
            Operation::MaintenanceLocation => "maintenance.location",
            Operation::TasksDispatch => "tasks.dispatch",
            Operation::TasksClaimNext => "tasks.claimNext",
        }
    }

    /// Every method, in table order.
    pub fn all() -> impl Iterator<Item = Operation> {
        [
            Operation::GraphCreateEntities,
            Operation::GraphAddObservations,
            Operation::GraphCreateRelations,
            Operation::GraphDeleteRelations,
            Operation::GraphDeleteEntities,
            Operation::GraphDeleteObservations,
            Operation::GraphDeleteObservationById,
            Operation::GraphUpdateObservationById,
            Operation::GraphUpdateObservation,
            Operation::GraphTruthUpsert,
            Operation::GraphTruthDelete,
            Operation::GraphReadGraph,
            Operation::GraphReadGraphFull,
            Operation::GraphOpenNodes,
            Operation::SearchNodes,
            Operation::MaintenanceStats,
            Operation::MaintenanceStatsPerEntity,
            Operation::MaintenancePurge,
            Operation::MaintenanceReset,
            Operation::MaintenanceCapabilities,
            Operation::MaintenanceHealth,
            Operation::MaintenanceLocation,
            Operation::TasksDispatch,
            Operation::TasksClaimNext,
        ]
        .into_iter()
    }
}
