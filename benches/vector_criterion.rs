// storage-boundary: provider-test
use asobi::storage::libsql::{
    db::init_db,
    vector::{Chunk, VectorStore},
};
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::env;
use std::hint::black_box;
use tempfile::tempdir;

const DIMENSION: usize = 768;

fn vector_hot_paths(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    let chunk_count = env::var("ASOBI_VECTOR_BENCH_SIZE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1_000);
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("vector-criterion.db");
    unsafe {
        env::set_var(
            asobi::paths::ENV_DATABASE_URL,
            db_path.to_str().expect("utf-8 path"),
        );
    }
    let store = runtime.block_on(async {
        let (_db, conn) = init_db().await.expect("init db");
        let store = VectorStore::new_with_dim(conn, DIMENSION);
        store
            .insert_chunks(make_chunks(chunk_count))
            .await
            .expect("seed chunks");
        store
    });
    let query = vec![0.1f32; DIMENSION];

    let mut group = c.benchmark_group("vector_hot_paths");
    group.sample_size(30);
    group.bench_with_input(
        BenchmarkId::new("search_top_5", chunk_count),
        &chunk_count,
        |b, _| {
            b.to_async(&runtime).iter(|| async {
                black_box(
                    store
                        .search(black_box(&query), 5)
                        .await
                        .expect("vector search"),
                );
            });
        },
    );
    group.finish();
}

fn make_chunks(count: usize) -> Vec<Chunk> {
    (0..count)
        .map(|i| {
            let mut vector = vec![0.0f32; DIMENSION];
            vector[i % DIMENSION] = 1.0;
            Chunk {
                id: uuid::Uuid::now_v7().to_string(),
                topic_id: format!("topic-{}", i % 10),
                chunk_idx: (i / 10) as u32,
                text: format!("vector benchmark chunk {i}"),
                source: "bench.md".into(),
                vector,
            }
        })
        .collect()
}

criterion_group!(benches, vector_hot_paths);
criterion_main!(benches);
