use asobi::api::{GraphStore, TaskStore};
use asobi::model::EntityInput;
use asobi::storage::Storage;
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use tempfile::tempdir;

fn task_hot_paths(c: &mut Criterion) {
    let dir = tempdir().expect("tempdir");
    let store = Storage::open_at(&dir.path().join("tasks.db")).expect("open storage");
    store
        .create_entities(vec![EntityInput {
            name: "task-1".into(),
            entity_type: "task".into(),
            observations: vec![],
        }])
        .unwrap();
    store
        .truth_upsert("task-1", "status", "READY_TO_DISPATCH")
        .unwrap();
    c.bench_function("task_claim_compare_and_set", |b| {
        b.iter(|| {
            store
                .transition("task-1", "DISPATCHED", "READY_TO_DISPATCH")
                .unwrap();
            black_box(store.claim_next("bench-agent").unwrap());
        })
    });
}

criterion_group!(benches, task_hot_paths);
criterion_main!(benches);
