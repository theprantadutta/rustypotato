//! Storage-layer microbenchmark.
//!
//! **NOT a network benchmark.** This binary calls `MemoryStore`
//! methods directly in-process — no TCP socket, no RESP codec, no
//! command dispatch, no AOF logging. The numbers it produces only
//! tell you about the raw `DashMap`/HashMap path, NOT what real
//! Redis-compatible clients will observe over the wire. For that,
//! use `cargo run --release --bin network_bench` (Stage 12), which
//! goes through the full TCP+RESP path with multiple concurrent
//! clients.
//!
//! Use this binary when you want to:
//! - Profile an internal storage change in isolation (e.g. comparing
//!   `DashMap` shard sizes, or a new value-type representation).
//! - Sanity-check that the storage layer hasn't regressed by orders
//!   of magnitude after a refactor.
//!
//! Run with: `cargo run --release --bin storage_microbench`

use rustypotato::MemoryStore;
use std::sync::Arc;
use std::time::{Duration, Instant};

const ITERATIONS: usize = 100_000;
const WARMUP_ITERATIONS: usize = 10_000;

fn main() {
    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║   RustyPotato Storage Microbenchmark (IN-PROCESS, NO NET)    ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();
    println!("⚠️  These numbers reflect raw `MemoryStore` calls only —");
    println!("    NO TCP, NO RESP, NO dispatch overhead. Do NOT compare");
    println!("    against `redis-benchmark`. Use `network_bench` for that.");
    println!();

    let rt = tokio::runtime::Runtime::new().unwrap();

    rt.block_on(async {
        let store = Arc::new(MemoryStore::new());

        println!("Configuration:");
        println!("  Iterations:    {:>10}", format_number(ITERATIONS));
        println!("  Warmup:        {:>10}", format_number(WARMUP_ITERATIONS));
        println!();

        // Warmup
        print!("Warming up...");
        for i in 0..WARMUP_ITERATIONS {
            let key = format!("warmup_key_{}", i);
            store
                .set(key.clone(), format!("value_{}", i))
                .await
                .unwrap();
            let _ = store.get(&key);
        }
        println!(" done");
        println!();

        // SET Benchmark
        println!("Running SET benchmark...");
        let set_latencies = benchmark_set(&store, ITERATIONS).await;
        let set_stats = calculate_stats(&set_latencies);

        // GET Benchmark (existing keys)
        println!("Running GET benchmark...");
        let get_latencies = benchmark_get(&store, ITERATIONS).await;
        let get_stats = calculate_stats(&get_latencies);

        // INCR Benchmark
        println!("Running INCR benchmark...");
        let incr_latencies = benchmark_incr(&store, ITERATIONS).await;
        let incr_stats = calculate_stats(&incr_latencies);

        // DELETE Benchmark
        println!("Running DELETE benchmark...");
        let del_latencies = benchmark_delete(&store, ITERATIONS).await;
        let del_stats = calculate_stats(&del_latencies);

        println!();
        println!("═══════════════════════════════════════════════════════════════");
        println!();
        println!("STORAGE-LAYER THROUGHPUT (in-process, single thread):");
        println!(
            "  SET operations:    {:>12} ops/sec",
            format_number(set_stats.ops_per_sec)
        );
        println!(
            "  GET operations:    {:>12} ops/sec",
            format_number(get_stats.ops_per_sec)
        );
        println!(
            "  INCR operations:   {:>12} ops/sec",
            format_number(incr_stats.ops_per_sec)
        );
        println!(
            "  DELETE operations: {:>12} ops/sec",
            format_number(del_stats.ops_per_sec)
        );
        println!();
        println!("LATENCY (SET):");
        println!("  Average:  {:>10}", format_duration(set_stats.avg));
        println!("  p50:      {:>10}", format_duration(set_stats.p50));
        println!("  p95:      {:>10}", format_duration(set_stats.p95));
        println!("  p99:      {:>10}", format_duration(set_stats.p99));
        println!("  Min:      {:>10}", format_duration(set_stats.min));
        println!("  Max:      {:>10}", format_duration(set_stats.max));
        println!();
        println!("LATENCY (GET):");
        println!("  Average:  {:>10}", format_duration(get_stats.avg));
        println!("  p50:      {:>10}", format_duration(get_stats.p50));
        println!("  p95:      {:>10}", format_duration(get_stats.p95));
        println!("  p99:      {:>10}", format_duration(get_stats.p99));
        println!("  Min:      {:>10}", format_duration(get_stats.min));
        println!("  Max:      {:>10}", format_duration(get_stats.max));
        println!();
        println!("═══════════════════════════════════════════════════════════════");
        println!();
        println!("For network-traffic numbers (50 clients, pipelined RESP):");
        println!("    cargo run --release --bin network_bench");
        println!();
        println!("For statistical detail (criterion, HTML reports):");
        println!("    cargo bench --bench performance_benchmarks");
        println!("    open target/criterion/report/index.html");
        println!();
    });
}

async fn benchmark_set(store: &Arc<MemoryStore>, iterations: usize) -> Vec<Duration> {
    let mut latencies = Vec::with_capacity(iterations);

    for i in 0..iterations {
        let key = format!("bench_key_{}", i);
        let value = format!("bench_value_{}", i);

        let start = Instant::now();
        store.set(key, value).await.unwrap();
        latencies.push(start.elapsed());
    }

    latencies
}

async fn benchmark_get(store: &Arc<MemoryStore>, iterations: usize) -> Vec<Duration> {
    let mut latencies = Vec::with_capacity(iterations);

    for i in 0..iterations {
        let key = format!("bench_key_{}", i);

        let start = Instant::now();
        let _ = store.get(&key);
        latencies.push(start.elapsed());
    }

    latencies
}

async fn benchmark_incr(store: &Arc<MemoryStore>, iterations: usize) -> Vec<Duration> {
    let mut latencies = Vec::with_capacity(iterations);

    // Pre-create a counter
    store
        .set("incr_counter".to_string(), "0".to_string())
        .await
        .unwrap();

    for _ in 0..iterations {
        let start = Instant::now();
        let _ = store.incr("incr_counter").await;
        latencies.push(start.elapsed());
    }

    latencies
}

async fn benchmark_delete(store: &Arc<MemoryStore>, iterations: usize) -> Vec<Duration> {
    let mut latencies = Vec::with_capacity(iterations);

    // Pre-create keys to delete
    for i in 0..iterations {
        let key = format!("del_key_{}", i);
        store.set(key, "value".to_string()).await.unwrap();
    }

    for i in 0..iterations {
        let key = format!("del_key_{}", i);

        let start = Instant::now();
        let _ = store.delete(&key).await;
        latencies.push(start.elapsed());
    }

    latencies
}

struct Stats {
    ops_per_sec: usize,
    avg: Duration,
    p50: Duration,
    p95: Duration,
    p99: Duration,
    min: Duration,
    max: Duration,
}

fn calculate_stats(latencies: &[Duration]) -> Stats {
    let mut sorted = latencies.to_vec();
    sorted.sort();

    let total: Duration = latencies.iter().sum();
    let avg = total / latencies.len() as u32;

    let p50 = sorted[sorted.len() * 50 / 100];
    let p95 = sorted[sorted.len() * 95 / 100];
    let p99 = sorted[sorted.len() * 99 / 100];
    let min = sorted[0];
    let max = sorted[sorted.len() - 1];

    let ops_per_sec = if avg.as_nanos() > 0 {
        (1_000_000_000 / avg.as_nanos()) as usize
    } else {
        0
    };

    Stats {
        ops_per_sec,
        avg,
        p50,
        p95,
        p99,
        min,
        max,
    }
}

fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.insert(0, ',');
        }
        result.insert(0, c);
    }
    result
}

fn format_duration(d: Duration) -> String {
    let nanos = d.as_nanos();
    if nanos < 1_000 {
        format!("{} ns", nanos)
    } else if nanos < 1_000_000 {
        format!("{:.2} µs", nanos as f64 / 1_000.0)
    } else if nanos < 1_000_000_000 {
        format!("{:.2} ms", nanos as f64 / 1_000_000.0)
    } else {
        format!("{:.2} s", nanos as f64 / 1_000_000_000.0)
    }
}
