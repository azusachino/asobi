use asobi_core::api::{GraphStore, MaintenanceStore, OpenNodes, SearchQuery, SearchStore};
use asobi_storage::SqliteStore;
use std::env;
use std::hint::black_box;
use std::time::Instant;
use tempfile::tempdir;

fn main() {
    let count = env::var("ASOBI_BENCH_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_000);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    runtime.block_on(run(count));
}

async fn run(count: usize) {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open_at(&dir.path().join("graph.db"))
        .await
        .expect("open storage");
    seed(&store, count).await;
    let start = Instant::now();
    for _ in 0..50 {
        black_box(
            store
                .search_nodes(SearchQuery {
                    query: "commonterm".into(),
                    limit: 10,
                    filters: vec![],
                })
                .await
                .unwrap(),
        );
    }
    println!("search: {:?}", start.elapsed() / 50);
    let start = Instant::now();
    for _ in 0..100 {
        black_box(
            store
                .open_nodes(OpenNodes {
                    names: vec!["entity-10".into()],
                    ..Default::default()
                })
                .await
                .unwrap(),
        );
    }
    println!("open: {:?}", start.elapsed() / 100);
    println!("stats: {:?}", store.stats().await.expect("stats"));
}

async fn seed(store: &impl GraphStore, count: usize) {
    store
        .create_entities(
            (0..count)
                .map(|i| asobi_core::model::EntityInput {
                    name: format!("entity-{i}"),
                    entity_type: "bench".into(),
                    observations: vec![format!("commonterm observation {i}")],
                })
                .collect(),
        )
        .await
        .expect("seed");
}
