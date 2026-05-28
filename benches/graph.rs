use rosemary::{db, mcp};
use std::env;
use std::hint::black_box;
use std::time::{Duration, Instant};
use tempfile::tempdir;

const DEFAULT_ENTITY_COUNT: usize = 10_000;
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

    runtime.block_on(async {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("bench.db");
        unsafe {
            std::env::set_var("DATABASE_URL", db_path.to_str().expect("utf-8 path"));
        }
        let (_db, conn) = db::init_db().await.expect("init db");

        let entity_count = env_usize("ROSEMARY_BENCH_ENTITIES", DEFAULT_ENTITY_COUNT);
        seed_graph(&conn, entity_count).await;

        let selective_hits = db::mcp_search_nodes(&conn, "rareterm7777")
            .await
            .expect("selective search")
            .entities
            .len();
        assert!(selective_hits > 0, "selective search returned no hits");
        let selective_elapsed = time_many(SEARCH_ITERS, || async {
            let graph = db::mcp_search_nodes(&conn, black_box("rareterm7777"))
                .await
                .expect("selective search");
            black_box(graph.entities.len());
        })
        .await;

        let capped_hits = db::mcp_search_nodes(&conn, "commonterm")
            .await
            .expect("broad capped search")
            .entities
            .len();
        assert_eq!(
            capped_hits,
            db::DEFAULT_SEARCH_LIMIT,
            "default broad search should be capped"
        );
        let broad_elapsed = time_many(BROAD_SEARCH_ITERS, || async {
            let graph = db::mcp_search_nodes(&conn, black_box("commonterm"))
                .await
                .expect("broad capped search");
            black_box(graph.entities.len());
        })
        .await;

        let export_hits = db::mcp_search_nodes_with_limit(&conn, "commonterm", entity_count)
            .await
            .expect("broad export search")
            .entities
            .len();
        assert_eq!(
            export_hits, entity_count,
            "explicit broad export should return every seeded entity"
        );
        let export_elapsed = time_many(BROAD_EXPORT_ITERS, || async {
            let graph =
                db::mcp_search_nodes_with_limit(&conn, black_box("commonterm"), entity_count)
                    .await
                    .expect("broad export search");
            black_box(graph.entities.len());
        })
        .await;

        let open_elapsed = time_many(OPEN_ITERS, || async {
            let graph = db::mcp_open_nodes(
                &conn,
                black_box(vec![
                    "entity-10".to_string(),
                    format!("entity-{}", entity_count / 2),
                    format!("entity-{}", entity_count - 1),
                ]),
            )
            .await
            .expect("open nodes");
            black_box(graph.entities.len());
        })
        .await;

        let stats_elapsed = time_many(STATS_ITERS, || async {
            let stats = db::mcp_stats(&conn).await.expect("stats");
            black_box(stats);
        })
        .await;

        println!(
            "seeded graph: entities={}, observations={}",
            entity_count, entity_count
        );
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
        db::mcp_reset(&conn).await.expect("reset");
        let reset_elapsed = reset_start.elapsed();
        println!("reset: total={:?}", reset_elapsed);
    });
}

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

async fn seed_graph(conn: &libsql::Connection, entity_count: usize) {
    for i in 0..entity_count {
        let rare_token = if i % 1_000 == 0 || i == 7_777 {
            " rareterm7777"
        } else {
            ""
        };
        db::mcp_create_entities(
            conn,
            vec![mcp::EntityInput {
                name: format!("entity-{i}"),
                entity_type: "bench".to_string(),
                observations: vec![format!(
                    "commonterm graph observation {i} with ranked full text search{rare_token}"
                )],
            }],
        )
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
