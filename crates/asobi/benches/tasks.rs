use asobi::api::{GraphStore, TaskStore};
use asobi::model::EntityInput;
use asobi::storage::Storage;
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use tempfile::tempdir;

fn task_hot_paths(c: &mut Criterion) {
    let dir = tempdir().expect("tempdir");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let store = runtime.block_on(async {
        let store = Storage::open_at(&dir.path().join("tasks.db"))
            .await
            .expect("open storage");
        store
            .create_entities(vec![EntityInput {
                name: "task-1".into(),
                entity_type: "task".into(),
                observations: vec![],
            }])
            .await
            .unwrap();
        store
            .truth_upsert("task-1", "status", "DISPATCHED")
            .await
            .unwrap();
        store
    });
    c.bench_function("task_claim_compare_and_set", |b| {
        b.iter(|| {
            runtime.block_on(async {
                store
                    .truth_upsert("task-1", "status", "READY_TO_DISPATCH")
                    .await
                    .unwrap();
                black_box(store.claim_next("bench-agent").await.unwrap());
            })
        })
    });
}

criterion_group!(benches, task_hot_paths);
criterion_main!(benches);
