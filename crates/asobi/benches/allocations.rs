use asobi::api::{GraphStore, OpenNodes, SearchQuery, SearchStore};
use asobi::storage::Storage;
use std::env;
use std::hint::black_box;
use tempfile::tempdir;

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

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
    let store = Storage::open_at(&dir.path().join("allocations.db"))
        .await
        .expect("open storage");
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
        .await
        .expect("seed");
    let _profiler = dhat::Profiler::new_heap();
    black_box(
        store
            .search_nodes(SearchQuery {
                query: "commonterm".into(),
                limit: count,
                filters: vec![],
            })
            .await
            .expect("search"),
    );
    black_box(
        store
            .open_nodes(OpenNodes {
                names: vec!["entity-10".into()],
                ..Default::default()
            })
            .await
            .expect("open"),
    );
}
