//! `asobi-server` — placeholder until WP4 wires the real server (ADR 0005,
//! ADR 0009). Exists so the crate, its binary and its release wiring exist.
fn main() {
    println!(
        "asobi-server {} (server not implemented yet; see ADR 0005 / plan WP4)",
        env!("CARGO_PKG_VERSION")
    );
}
