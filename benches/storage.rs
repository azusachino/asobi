use asobi::api::{GraphStore, OpenNodes, SearchQuery, SearchStore};
use asobi::model::EntityInput;
use asobi::storage::SqliteStore;
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use tempfile::tempdir;

fn storage_hot_paths(c: &mut Criterion) {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open_at(&dir.path().join("bench.db")).expect("open storage");
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
        .expect("seed graph");

    c.bench_function("sqlite_fts_search", |b| {
        b.iter(|| {
            black_box(
                store
                    .search_nodes(SearchQuery {
                        query: black_box("commonterm").into(),
                        limit: 20,
                        filters: Vec::new(),
                    })
                    .expect("search"),
            )
        })
    });
    c.bench_function("sqlite_open_nodes", |b| {
        b.iter(|| {
            black_box(
                store
                    .open_nodes(OpenNodes {
                        names: vec!["entity-10".into(), "entity-999".into()],
                        ..Default::default()
                    })
                    .expect("open nodes"),
            )
        })
    });
}

criterion_group!(benches, storage_hot_paths);
criterion_main!(benches);
