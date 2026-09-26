use asobi::api::{GraphStore, MaintenanceStore, OpenNodes, SearchQuery, SearchStore};
use asobi::model::EntityInput;
use asobi::storage::SqliteStore;
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use tempfile::tempdir;

fn storage_hot_paths(c: &mut Criterion) {
    let dir = tempdir().expect("tempdir");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let store = runtime.block_on(async {
        let store = SqliteStore::open_at(&dir.path().join("bench.db"))
            .await
            .expect("open storage");
        store
            .create_entities(
                (0..1_000)
                    .map(|i| EntityInput {
                        name: format!("entity-{i}"),
                        entity_type: "bench".into(),
                        observations: vec![format!("commonterm observation {i}")],
                    })
                    .collect(),
            )
            .await
            .expect("seed graph");
        store
    });

    c.bench_function("sqlite_fts_search", |b| {
        b.iter(|| {
            runtime.block_on(async {
                black_box(
                    store
                        .search_nodes(SearchQuery {
                            query: black_box("commonterm").into(),
                            limit: 20,
                            filters: Vec::new(),
                        })
                        .await
                        .expect("search"),
                )
            })
        })
    });
    c.bench_function("sqlite_open_nodes", |b| {
        b.iter(|| {
            runtime.block_on(async {
                black_box(
                    store
                        .open_nodes(OpenNodes {
                            names: vec!["entity-10".into(), "entity-999".into()],
                            ..Default::default()
                        })
                        .await
                        .expect("open nodes"),
                )
            })
        })
    });
}

criterion_group!(benches, storage_hot_paths);
criterion_main!(benches);
