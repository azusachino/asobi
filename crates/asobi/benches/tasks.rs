use asobi::storage::Storage;
use asobi_core::api::{GraphStore, TaskStore};
use asobi_core::model::EntityInput;
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::tempdir;

fn task_hot_paths(c: &mut Criterion) {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("tasks.db");
    let graph = format!(
        "bench-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    // The benchmark uses the same provider selection as the CLI. With
    // ASOBI_REMOTE set and `REMOTE=1`, this measures the server-backed path.
    unsafe {
        std::env::set_var("ASOBI_DATABASE_URL", db_path);
        std::env::set_var("ASOBI_GRAPH", graph);
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let store = runtime.block_on(async {
        let store = Storage::open_default().await.expect("open storage");
        store
            .create_entities(vec![EntityInput {
                name: "bench:task-1".into(),
                entity_type: "task".into(),
                observations: vec![],
            }])
            .await
            .unwrap();
        store
            .truth_upsert("bench:task-1", "status", "DISPATCHED")
            .await
            .unwrap();
        store
    });
    c.bench_function("task_claim_compare_and_set", |b| {
        b.iter(|| {
            runtime.block_on(async {
                store
                    .truth_upsert("bench:task-1", "status", "READY_TO_DISPATCH")
                    .await
                    .unwrap();
                black_box(store.claim_next("bench-agent").await.unwrap());
            })
        })
    });
}

criterion_group!(benches, task_hot_paths);
criterion_main!(benches);
