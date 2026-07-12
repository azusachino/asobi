#[cfg(not(feature = "turso-experimental"))]
use asobi::api::{GraphStore, OpenNodes, SearchQuery, SearchStore};
#[cfg(not(feature = "turso-experimental"))]
use asobi::storage::Storage;
#[cfg(not(feature = "turso-experimental"))]
use std::{env, hint::black_box};
#[cfg(not(feature = "turso-experimental"))]
use tempfile::tempdir;

#[cfg(not(feature = "turso-experimental"))]
#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

#[cfg(not(feature = "turso-experimental"))]
fn main() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    let entity_count = env::var("ASOBI_BENCH_SIZE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1_000);
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("allocations.db");
    unsafe {
        env::set_var("ASOBI_DATABASE_URL", db_path.to_str().expect("utf-8 path"));
    }
    let store = runtime.block_on(async {
        let store = Storage::open_default().await.expect("open storage");
        let entities = (0..entity_count)
            .map(|i| asobi::model::EntityInput {
                name: format!("entity-{i}"),
                entity_type: "bench".into(),
                observations: vec![format!("commonterm allocation observation {i}")],
            })
            .collect();
        store.create_entities(entities).await.expect("seed graph");
        store
    });

    let _profiler = dhat::Profiler::new_heap();
    runtime.block_on(async {
        black_box(
            store
                .search_nodes(SearchQuery {
                    query: "commonterm".into(),
                    limit: entity_count,
                    filters: Vec::new(),
                })
                .await
                .expect("broad search"),
        );
        black_box(
            store
                .open_nodes(OpenNodes {
                    names: vec!["entity-10".into(), format!("entity-{}", entity_count - 1)],
                    with_ids: false,
                    expand: Vec::new(),
                })
                .await
                .expect("open nodes"),
        );
    });
}

#[cfg(feature = "turso-experimental")]
fn main() {
    eprintln!("allocation profiling is libSQL-only; omit turso-experimental");
}
