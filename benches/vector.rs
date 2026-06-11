#[cfg(feature = "documents")]
use rosemary::{
    db::init_db,
    vector::{Chunk, VectorStore},
};
#[cfg(feature = "documents")]
use std::hint::black_box;
#[cfg(feature = "documents")]
use std::time::Instant;
#[cfg(feature = "documents")]
use tempfile::tempdir;

#[cfg(feature = "documents")]
const INSERT_SIZE: usize = 1000;
#[cfg(feature = "documents")]
const SEARCH_ITERS: usize = 100;
#[cfg(feature = "documents")]
const DIMENSION: usize = 384;

#[cfg(feature = "documents")]
fn main() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        println!("\n=== Vector Store Benchmarks (libSQL Vector Search) ===");
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("bench_vector.db");
        unsafe {
            std::env::set_var(
                "ROSEMARY_DATABASE_URL",
                db_path.to_str().expect("utf-8 path"),
            );
        }
        let (_db, conn) = init_db().await.expect("init db");
        let store = VectorStore::new_with_dim(conn.clone(), DIMENSION);

        // Benchmark insertion of 1000 chunks
        let mut chunks = Vec::with_capacity(INSERT_SIZE);
        for i in 0..INSERT_SIZE {
            let mut vector = vec![0.0f32; DIMENSION];
            vector[i % DIMENSION] = 1.0f32;
            chunks.push(Chunk {
                id: uuid::Uuid::new_v4().to_string(),
                topic_id: format!("topic-{}", i % 10),
                chunk_idx: (i / 10) as u32,
                text: format!("vector benchmark chunk text {}", i),
                source: "bench.md".to_string(),
                vector,
            });
        }

        let insert_start = Instant::now();
        store.insert_chunks(chunks).await.expect("insert chunks");
        let insert_elapsed = insert_start.elapsed();
        println!(
            "insert_chunks: total={:?}, avg={:?} per chunk, count={}",
            insert_elapsed,
            insert_elapsed / INSERT_SIZE as u32,
            INSERT_SIZE
        );

        // Benchmark searching
        let query_vector = vec![0.1f32; DIMENSION];
        let search_start = Instant::now();
        for _ in 0..SEARCH_ITERS {
            let results = store
                .search(black_box(&query_vector), black_box(5))
                .await
                .expect("search");
            black_box(results.len());
        }
        let search_elapsed = search_start.elapsed();
        println!(
            "search (top-5): total={:?}, avg={:?} per query, iters={}",
            search_elapsed,
            search_elapsed / SEARCH_ITERS as u32,
            SEARCH_ITERS
        );
    });
}

#[cfg(not(feature = "documents"))]
fn main() {
    println!("Vector benchmark requires the 'documents' feature.");
}
