use miku::skills::{derive_source_slug, parse_frontmatter};
use std::time::Instant;

fn main() {
    println!("=== Skills Subsystem Micro-benchmarks ===");

    // 1. parse_frontmatter benchmarking
    let frontmatter_inputs = vec![
        (
            "---\nname: test-skill\ndescription: \"some skill\"\n---\nbody content",
            "full frontmatter",
        ),
        (
            "---\ndescription: \"no name\"\n---\nbody content",
            "missing name",
        ),
        (
            "---\nname: test-skill\n---\nbody content",
            "missing description",
        ),
        (
            "---\r\nname: crlf-skill\r\ndescription: \"crlf\"\r\n---\r\nbody content",
            "crlf frontmatter",
        ),
    ];

    for (input, label) in frontmatter_inputs {
        let iters = 100_000;
        let start = Instant::now();
        for _ in 0..iters {
            let _ = parse_frontmatter(input);
        }
        let elapsed = start.elapsed();
        println!(
            "parse_frontmatter ({:<20}) | avg: {:?}, iters: {}",
            label,
            elapsed / iters,
            iters
        );
    }

    // 2. derive_source_slug benchmarking
    let slug_inputs = vec![
        "https://github.com/jasonswett/llm-skills.git",
        "git@github.com:jasonswett/llm-skills.git",
        "/path/to/local-skills",
    ];

    for input in slug_inputs {
        let iters = 100_000;
        let start = Instant::now();
        for _ in 0..iters {
            let _ = derive_source_slug(input);
        }
        let elapsed = start.elapsed();
        println!(
            "derive_source_slug ({:<45}) | avg: {:?}, iters: {}",
            input,
            elapsed / iters,
            iters
        );
    }
}
