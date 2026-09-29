use asobi::storage::RemoteStore;
use asobi_server::{App, BoundServer};
use std::sync::Arc;
use tempfile::tempdir;

#[path = "../../asobi-core/tests/support/backend_api_contract.rs"]
mod shared_contract;

async fn remote_store(graph: &str) -> (tempfile::TempDir, BoundServer, RemoteStore) {
    let data = tempdir().unwrap();
    let server = BoundServer::bind(
        Arc::new(App::new(data.path().to_path_buf())),
        "127.0.0.1:0".parse().unwrap(),
    )
    .await
    .unwrap();
    let store = RemoteStore::new(format!("http://{}", server.local_addr), graph.into()).unwrap();
    (data, server, store)
}

macro_rules! remote_contract_test {
    ($test_name:ident, $graph:literal, $contract:ident) => {
        #[tokio::test]
        async fn $test_name() {
            let (_data, server, store) = remote_store($graph).await;
            shared_contract::$contract(&store).await;
            server.shutdown().await;
        }
    };
}

remote_contract_test!(
    remote_implements_the_v3_contract,
    "contract-capabilities",
    stats_capabilities_health_and_location_contract
);
remote_contract_test!(
    remote_graph_truth_search_and_task_claim_are_atomic_surfaces,
    "contract-claims",
    graph_truth_search_and_task_claim_are_atomic_surfaces
);
remote_contract_test!(
    remote_task_update_is_one_operation_and_preserves_status_without_explicit_change,
    "contract-task-update",
    task_update_is_one_operation_and_preserves_status_without_explicit_change
);
remote_contract_test!(
    remote_graph_and_search_keep_observations_lazy,
    "contract-lazy",
    graph_and_search_keep_observations_lazy
);
remote_contract_test!(
    remote_purge_is_preview_first_and_leaves_durable_knowledge,
    "contract-purge",
    purge_is_preview_first_and_leaves_durable_knowledge
);
remote_contract_test!(
    remote_show_returns_recent_observations_and_the_true_total,
    "contract-show",
    show_returns_recent_observations_and_the_true_total
);
remote_contract_test!(
    remote_search_reaches_truth_values_not_just_observations,
    "contract-search-truth",
    search_reaches_truth_values_not_just_observations
);
remote_contract_test!(
    remote_search_widens_rather_than_returning_a_silent_zero,
    "contract-search-widen",
    search_widens_rather_than_returning_a_silent_zero
);
remote_contract_test!(
    remote_search_returns_results_in_ranked_order_not_alphabetical,
    "contract-search-ranking",
    search_returns_results_in_ranked_order_not_alphabetical
);
