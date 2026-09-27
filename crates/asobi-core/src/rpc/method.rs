//! The RPC method names, one per `v3` trait method plus the handshake.
//!
//! Names are `<trait>.<method>` in camelCase, exactly as the ADR's table
//! lists them; this enum is the single parser for the path segment, and the
//! schema publisher walks [`Method::all`] so a new method cannot be added
//! without its schema row.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    ServerHello,
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

impl Method {
    pub fn parse(name: &str) -> Option<Method> {
        Some(match name {
            "server.hello" => Method::ServerHello,
            "graph.createEntities" => Method::GraphCreateEntities,
            "graph.addObservations" => Method::GraphAddObservations,
            "graph.createRelations" => Method::GraphCreateRelations,
            "graph.deleteRelations" => Method::GraphDeleteRelations,
            "graph.deleteEntities" => Method::GraphDeleteEntities,
            "graph.deleteObservations" => Method::GraphDeleteObservations,
            "graph.deleteObservationById" => Method::GraphDeleteObservationById,
            "graph.updateObservationById" => Method::GraphUpdateObservationById,
            "graph.updateObservation" => Method::GraphUpdateObservation,
            "graph.truthUpsert" => Method::GraphTruthUpsert,
            "graph.truthDelete" => Method::GraphTruthDelete,
            "graph.readGraph" => Method::GraphReadGraph,
            "graph.readGraphFull" => Method::GraphReadGraphFull,
            "graph.openNodes" => Method::GraphOpenNodes,
            "search.nodes" => Method::SearchNodes,
            "maintenance.stats" => Method::MaintenanceStats,
            "maintenance.statsPerEntity" => Method::MaintenanceStatsPerEntity,
            "maintenance.purge" => Method::MaintenancePurge,
            "maintenance.reset" => Method::MaintenanceReset,
            "maintenance.capabilities" => Method::MaintenanceCapabilities,
            "maintenance.health" => Method::MaintenanceHealth,
            "maintenance.location" => Method::MaintenanceLocation,
            "tasks.dispatch" => Method::TasksDispatch,
            "tasks.claimNext" => Method::TasksClaimNext,
            _ => return None,
        })
    }

    /// The wire name, the inverse of [`Method::parse`].
    pub fn name(&self) -> &'static str {
        match self {
            Method::ServerHello => "server.hello",
            Method::GraphCreateEntities => "graph.createEntities",
            Method::GraphAddObservations => "graph.addObservations",
            Method::GraphCreateRelations => "graph.createRelations",
            Method::GraphDeleteRelations => "graph.deleteRelations",
            Method::GraphDeleteEntities => "graph.deleteEntities",
            Method::GraphDeleteObservations => "graph.deleteObservations",
            Method::GraphDeleteObservationById => "graph.deleteObservationById",
            Method::GraphUpdateObservationById => "graph.updateObservationById",
            Method::GraphUpdateObservation => "graph.updateObservation",
            Method::GraphTruthUpsert => "graph.truthUpsert",
            Method::GraphTruthDelete => "graph.truthDelete",
            Method::GraphReadGraph => "graph.readGraph",
            Method::GraphReadGraphFull => "graph.readGraphFull",
            Method::GraphOpenNodes => "graph.openNodes",
            Method::SearchNodes => "search.nodes",
            Method::MaintenanceStats => "maintenance.stats",
            Method::MaintenanceStatsPerEntity => "maintenance.statsPerEntity",
            Method::MaintenancePurge => "maintenance.purge",
            Method::MaintenanceReset => "maintenance.reset",
            Method::MaintenanceCapabilities => "maintenance.capabilities",
            Method::MaintenanceHealth => "maintenance.health",
            Method::MaintenanceLocation => "maintenance.location",
            Method::TasksDispatch => "tasks.dispatch",
            Method::TasksClaimNext => "tasks.claimNext",
        }
    }

    /// Every method, in table order.
    pub fn all() -> impl Iterator<Item = Method> {
        [
            Method::ServerHello,
            Method::GraphCreateEntities,
            Method::GraphAddObservations,
            Method::GraphCreateRelations,
            Method::GraphDeleteRelations,
            Method::GraphDeleteEntities,
            Method::GraphDeleteObservations,
            Method::GraphDeleteObservationById,
            Method::GraphUpdateObservationById,
            Method::GraphUpdateObservation,
            Method::GraphTruthUpsert,
            Method::GraphTruthDelete,
            Method::GraphReadGraph,
            Method::GraphReadGraphFull,
            Method::GraphOpenNodes,
            Method::SearchNodes,
            Method::MaintenanceStats,
            Method::MaintenanceStatsPerEntity,
            Method::MaintenancePurge,
            Method::MaintenanceReset,
            Method::MaintenanceCapabilities,
            Method::MaintenanceHealth,
            Method::MaintenanceLocation,
            Method::TasksDispatch,
            Method::TasksClaimNext,
        ]
        .into_iter()
    }
}
