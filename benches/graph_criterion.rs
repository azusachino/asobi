use asobi::api::{GraphStore, MaintenanceStore, OpenNodes, SearchQuery, SearchStore};
use asobi::storage::Storage;
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::env;
use std::hint::black_box;
use tempfile::tempdir;

fn graph_hot_paths(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    let entity_count = env::var("ASOBI_BENCH_SIZE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1_000);
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("criterion.db");
    unsafe {
        env::set_var("ASOBI_DATABASE_URL", db_path.to_str().expect("utf-8 path"));
    }
    let store = runtime.block_on(async {
        let store = Storage::open_default().await.expect("open storage");
        seed_graph(&store, entity_count).await;
        store
    });
    let relation_db_path = dir.path().join("criterion-relations.db");
    unsafe {
        env::set_var(
            "ASOBI_DATABASE_URL",
            relation_db_path.to_str().expect("utf-8 path"),
        );
    }
    let relation_store = runtime.block_on(async {
        let store = Storage::open_default()
            .await
            .expect("open relation storage");
        seed_relation_star(&store, entity_count).await;
        store
    });

    let mut group = c.benchmark_group("graph_hot_paths");
    group.sample_size(30);

    group.bench_with_input(
        BenchmarkId::new("search_selective", entity_count),
        &entity_count,
        |b, _| {
            b.to_async(&runtime).iter(|| async {
                black_box(
                    store
                        .search_nodes(SearchQuery {
                            query: black_box("rareterm7777").into(),
                            limit: 10,
                            filters: Vec::new(),
                        })
                        .await
                        .expect("selective search"),
                );
            });
        },
    );
    group.bench_with_input(
        BenchmarkId::new("search_broad_capped", entity_count),
        &entity_count,
        |b, _| {
            b.to_async(&runtime).iter(|| async {
                black_box(
                    store
                        .search_nodes(SearchQuery {
                            query: black_box("commonterm").into(),
                            limit: 10,
                            filters: Vec::new(),
                        })
                        .await
                        .expect("broad capped search"),
                );
            });
        },
    );
    group.bench_with_input(
        BenchmarkId::new("search_broad_full", entity_count),
        &entity_count,
        |b, count| {
            b.to_async(&runtime).iter(|| async {
                black_box(
                    store
                        .search_nodes(SearchQuery {
                            query: black_box("commonterm").into(),
                            limit: *count,
                            filters: Vec::new(),
                        })
                        .await
                        .expect("broad full search"),
                );
            });
        },
    );
    group.bench_function("search_truth_filter", |b| {
        b.to_async(&runtime).iter(|| async {
            black_box(
                store
                    .search_nodes(SearchQuery {
                        query: String::new(),
                        limit: 10,
                        filters: vec![("status".into(), "READY".into())],
                    })
                    .await
                    .expect("truth filter"),
            );
        });
    });
    group.bench_function("open_three", |b| {
        b.to_async(&runtime).iter(|| async {
            black_box(
                store
                    .open_nodes(OpenNodes {
                        names: vec![
                            "entity-10".into(),
                            format!("entity-{}", entity_count / 2),
                            format!("entity-{}", entity_count - 1),
                        ],
                        with_ids: false,
                        expand: Vec::new(),
                    })
                    .await
                    .expect("open nodes"),
            );
        });
    });
    group.bench_function("expand_relation_star", |b| {
        b.to_async(&runtime).iter(|| async {
            black_box(
                relation_store
                    .open_nodes(OpenNodes {
                        names: vec!["entity-0".into()],
                        with_ids: false,
                        expand: vec!["depends_on".into()],
                    })
                    .await
                    .expect("expand relation star"),
            );
        });
    });
    group.bench_function("stats", |b| {
        b.to_async(&runtime).iter(|| async {
            black_box(store.stats().await.expect("stats"));
        });
    });
    group.finish();
}

async fn seed_graph(store: &impl GraphStore, entity_count: usize) {
    let entities = (0..entity_count)
        .map(|i| {
            let rare = if i % 1_000 == 0 || i == 7_777 {
                " rareterm7777"
            } else {
                ""
            };
            asobi::model::EntityInput {
                name: format!("entity-{i}"),
                entity_type: "bench".into(),
                observations: vec![format!("commonterm graph observation {i}{rare}")],
            }
        })
        .collect();
    store.create_entities(entities).await.expect("seed graph");

    let truth_stride = (entity_count / 100).max(1);
    for i in (0..entity_count).step_by(truth_stride).take(100) {
        store
            .truth_upsert(&format!("entity-{i}"), "status", "READY")
            .await
            .expect("seed truth");
    }
}

async fn seed_relation_star(store: &impl GraphStore, entity_count: usize) {
    let entities = (0..entity_count)
        .map(|i| asobi::model::EntityInput {
            name: format!("entity-{i}"),
            entity_type: "bench".into(),
            observations: Vec::new(),
        })
        .collect();
    store
        .create_entities(entities)
        .await
        .expect("seed entities");
    let relations = (1..entity_count)
        .map(|i| asobi::model::RelationInput {
            from: "entity-0".into(),
            to: format!("entity-{i}"),
            relation_type: "depends_on".into(),
        })
        .collect();
    store
        .create_relations(relations)
        .await
        .expect("seed relations");
}

criterion_group!(benches, graph_hot_paths);
criterion_main!(benches);
