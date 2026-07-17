use asobi::api::{GraphStore, MaintenanceStore, OpenNodes, SearchQuery, SearchStore};
use asobi::storage::Storage;
use std::env;
use std::hint::black_box;
use std::time::Instant;
use tempfile::tempdir;

fn main() {
    let count = env::var("ASOBI_BENCH_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_000);
    let dir = tempdir().expect("tempdir");
    let store = Storage::open_at(&dir.path().join("graph.db")).expect("open storage");
    seed(&store, count);
    let start = Instant::now();
    for _ in 0..50 {
        black_box(
            store
                .search_nodes(SearchQuery {
                    query: "commonterm".into(),
                    limit: 10,
                    filters: vec![],
                })
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
                .unwrap(),
        );
    }
    println!("open: {:?}", start.elapsed() / 100);
    println!("stats: {:?}", store.stats().expect("stats"));
}

fn seed(store: &impl GraphStore, count: usize) {
    store
        .create_entities(
            (0..count)
                .map(|i| asobi::model::EntityInput {
                    name: format!("entity-{i}"),
                    entity_type: "bench".into(),
                    observations: vec![format!("commonterm observation {i}")],
                })
                .collect(),
        )
        .expect("seed");
}
