use asobi::normalize::normalize_key;
use std::time::Instant;

fn main() {
    let inputs = vec![
        "short",
        "User Preferences",
        "ame:mobile-support:task-1",
        "a very long string with many spaces and symbols : . _ - @ ! # $ % ^ & * ( )",
        "cli-日本語-'; DROP TABLE mcp_entities; --",
    ];

    println!("=== Normalization Micro-benchmarks ===");
    for input in inputs {
        let iters = 100_000;
        let start = Instant::now();
        for _ in 0..iters {
            let _ = normalize_key(input);
        }
        let elapsed = start.elapsed();
        println!(
            "input: {:<40} | avg: {:?}, iters: {}",
            input,
            elapsed / iters,
            iters
        );
    }
}
