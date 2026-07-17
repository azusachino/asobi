use asobi::api::{GraphStore, OpenNodes, SearchQuery, SearchStore};
use asobi::model::{EntityInput, RelationInput};
use asobi::storage::Storage;
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use tempfile::tempdir;

fn task_dispatcher_hot_paths(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("tasks-bench.db");
    unsafe {
        std::env::set_var("ASOBI_DATABASE_URL", db_path.to_str().expect("utf-8 path"));
    }
    let store = runtime.block_on(async {
        let store = Storage::open_default().await.expect("open storage");
        store
            .create_entities(vec![EntityInput {
                name: "bench:epic".into(),
                entity_type: "task".into(),
                observations: vec![],
            }])
            .await
            .expect("create epic");
        let mut children = Vec::new();
        let mut relations = Vec::new();
        for i in 0..100 {
            let name = format!("bench:epic:task-{}", i + 1);
            children.push(EntityInput {
                name: name.clone(),
                entity_type: "task".into(),
                observations: vec![],
            });
            relations.push(RelationInput {
                from: name,
                to: "bench:epic".into(),
                relation_type: "part_of".into(),
            });
        }
        store.create_entities(children).await.expect("create tasks");
        for i in 0..100 {
            store
                .truth_upsert(
                    &format!("bench:epic:task-{}", i + 1),
                    "status",
                    "READY_TO_DISPATCH",
                )
                .await
                .expect("seed task status");
        }
        store.create_relations(relations).await.expect("link tasks");
        store
    });

    c.bench_function("task_board_ready_search", |b| {
        b.to_async(&runtime).iter(|| async {
            let board = store
                .search_nodes(SearchQuery {
                    query: String::new(),
                    limit: 100,
                    filters: vec![("status".into(), "READY_TO_DISPATCH".into())],
                })
                .await
                .expect("ready task search");
            black_box(board.entities.len());
        });
    });
    c.bench_function("task_board_expand_epic", |b| {
        b.to_async(&runtime).iter(|| async {
            let board = store
                .open_nodes(OpenNodes {
                    names: vec!["bench:epic".into()],
                    expand: vec!["part_of".into()],
                    ..Default::default()
                })
                .await
                .expect("expand task board");
            black_box(board.entities.len());
        });
    });
}

criterion_group!(benches, task_dispatcher_hot_paths);
criterion_main!(benches);
