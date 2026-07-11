use asobi::api::{GraphStore, MaintenanceStore, OpenNodes, SearchQuery, SearchStore};
use asobi::storage::Storage;
use std::env;
use std::hint::black_box;
use std::time::{Duration, Instant};
use tempfile::tempdir;

const SEARCH_ITERS: usize = 50;
const BROAD_SEARCH_ITERS: usize = 5;
const BROAD_EXPORT_ITERS: usize = 3;
const STATS_ITERS: usize = 100;
const OPEN_ITERS: usize = 1_000;

fn main() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");

    let sizes_env =
        env::var("ASOBI_BENCH_SIZES").unwrap_or_else(|_| "1000,10000,50000".to_string());
    let sizes: Vec<usize> = sizes_env
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    if sizes.is_empty() {
        eprintln!("No valid bench sizes provided");
        return;
    }

    runtime.block_on(async {
        for entity_count in sizes {
            println!("\n=== Benchmarking with {} entities ===", entity_count);

            let dir = tempdir().expect("tempdir");
            let db_path = dir.path().join("bench.db");
            unsafe {
                std::env::set_var("ASOBI_DATABASE_URL", db_path.to_str().expect("utf-8 path"));
            }
            let store = Storage::open_default().await.expect("open default storage");

            seed_graph(&store, entity_count).await;

            let selective_hits = store
                .search_nodes(SearchQuery {
                    query: "rareterm7777".into(),
                    limit: 10,
                    filters: Vec::new(),
                })
                .await
                .expect("selective search")
                .entities
                .len();
            assert!(selective_hits > 0, "selective search returned no hits");
            let selective_elapsed = time_many(SEARCH_ITERS, || async {
                let graph = store
                    .search_nodes(SearchQuery {
                        query: black_box("rareterm7777").into(),
                        limit: 10,
                        filters: Vec::new(),
                    })
                    .await
                    .expect("selective search");
                black_box(graph.entities.len());
            })
            .await;

            let capped_hits = store
                .search_nodes(SearchQuery {
                    query: "commonterm".into(),
                    limit: 10,
                    filters: Vec::new(),
                })
                .await
                .expect("broad capped search")
                .entities
                .len();
            assert_eq!(capped_hits, 10, "default broad search should be capped");
            let broad_elapsed = time_many(BROAD_SEARCH_ITERS, || async {
                let graph = store
                    .search_nodes(SearchQuery {
                        query: black_box("commonterm").into(),
                        limit: 10,
                        filters: Vec::new(),
                    })
                    .await
                    .expect("broad capped search");
                black_box(graph.entities.len());
            })
            .await;

            let export_hits = store
                .search_nodes(SearchQuery {
                    query: "commonterm".into(),
                    limit: entity_count,
                    filters: Vec::new(),
                })
                .await
                .expect("broad export search")
                .entities
                .len();
            assert_eq!(
                export_hits, entity_count,
                "explicit broad export should return every seeded entity"
            );
            let export_elapsed = time_many(BROAD_EXPORT_ITERS, || async {
                let graph = store
                    .search_nodes(SearchQuery {
                        query: black_box("commonterm").into(),
                        limit: entity_count,
                        filters: Vec::new(),
                    })
                    .await
                    .expect("broad export search");
                black_box(graph.entities.len());
            })
            .await;

            let open_elapsed = time_many(OPEN_ITERS, || async {
                let graph = store
                    .open_nodes(OpenNodes {
                        names: black_box(vec![
                            "entity-10".to_string(),
                            format!("entity-{}", entity_count / 2),
                            format!("entity-{}", entity_count - 1),
                        ]),
                        with_ids: false,
                        expand: Vec::new(),
                    })
                    .await
                    .expect("open nodes");
                black_box(graph.entities.len());
            })
            .await;

            let stats_elapsed = time_many(STATS_ITERS, || async {
                let stats = store.stats().await.expect("stats");
                black_box(stats);
            })
            .await;

            println!(
                "search_nodes selective: total={:?}, avg={:?}, iters={}, hits={}",
                selective_elapsed,
                selective_elapsed / SEARCH_ITERS as u32,
                SEARCH_ITERS,
                selective_hits
            );
            println!(
                "search_nodes broad capped: total={:?}, avg={:?}, iters={}, hits={}",
                broad_elapsed,
                broad_elapsed / BROAD_SEARCH_ITERS as u32,
                BROAD_SEARCH_ITERS,
                capped_hits
            );
            println!(
                "search_nodes broad export: total={:?}, avg={:?}, iters={}, hits={}",
                export_elapsed,
                export_elapsed / BROAD_EXPORT_ITERS as u32,
                BROAD_EXPORT_ITERS,
                export_hits
            );
            println!(
                "open_nodes 3 names: total={:?}, avg={:?}, iters={}",
                open_elapsed,
                open_elapsed / OPEN_ITERS as u32,
                OPEN_ITERS
            );
            println!(
                "stats: total={:?}, avg={:?}, iters={}",
                stats_elapsed,
                stats_elapsed / STATS_ITERS as u32,
                STATS_ITERS
            );

            let reset_start = Instant::now();
            store.reset().await.expect("reset");
            let reset_elapsed = reset_start.elapsed();
            println!("reset: total={:?}", reset_elapsed);
        }
    });
}

async fn seed_graph(store: &impl GraphStore, entity_count: usize) {
    for i in 0..entity_count {
        let rare_token = if i % 1_000 == 0 || i == 7_777 {
            " rareterm7777"
        } else {
            ""
        };
        store
            .create_entities(vec![asobi::model::EntityInput {
                name: format!("entity-{i}"),
                entity_type: "bench".to_string(),
                observations: vec![format!(
                    "commonterm graph observation {i} with ranked full text search{rare_token}"
                )],
            }])
            .await
            .expect("create entity");
    }
}

async fn time_many<F, Fut>(iters: usize, mut f: F) -> Duration
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let start = Instant::now();
    for _ in 0..iters {
        f().await;
    }
    start.elapsed()
}
