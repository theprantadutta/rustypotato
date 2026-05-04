//! Network-end-to-end benchmark — the honest one.
//!
//! `tools/storage_microbench.rs` measures `MemoryStore` directly (no
//! TCP, no RESP, in-process). Those numbers reflect the storage layer
//! ceiling but are useless as a real "what does this server do for
//! clients" claim.
//!
//! This tool connects via real TCP, encodes commands as RESP, sends
//! pipelined batches across multiple concurrent connections, and reads
//! the matching responses. The numbers it produces are what a client
//! actually sees.
//!
//! Usage: `cargo run --release --bin network_bench`
//!
//! The server must already be running on the configured port (default
//! 6379). A typical session:
//!
//!   ./target/release/rustypotato-server --port 6379 &
//!   cargo run --release --bin network_bench
//!
//! Mirrors the shape of `redis-benchmark`: 50 concurrent clients, 100k
//! requests per command type, pipeline depth 16. Tweakable via env
//! vars: `BENCH_CLIENTS`, `BENCH_REQUESTS`, `BENCH_PIPELINE`,
//! `BENCH_ADDR`.

use std::env;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const DEFAULT_CLIENTS: usize = 50;
const DEFAULT_REQUESTS: usize = 100_000;
const DEFAULT_PIPELINE: usize = 16;

#[tokio::main]
async fn main() {
    let addr = env::var("BENCH_ADDR").unwrap_or_else(|_| "127.0.0.1:6379".to_string());
    let clients = parse_env_usize("BENCH_CLIENTS", DEFAULT_CLIENTS);
    let total_requests = parse_env_usize("BENCH_REQUESTS", DEFAULT_REQUESTS);
    let pipeline = parse_env_usize("BENCH_PIPELINE", DEFAULT_PIPELINE);

    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║      RustyPotato Network Benchmark (TCP + RESP, honest)      ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();
    println!("Target:           {addr}");
    println!("Concurrent clients: {clients}");
    println!("Requests / cmd:   {total_requests}");
    println!("Pipeline depth:   {pipeline}");
    println!();

    if !ping(&addr).await {
        eprintln!(
            "ERROR: cannot reach {addr}. Start the server first:\n  \
             ./target/release/rustypotato-server --port {} &",
            addr.split(':').next_back().unwrap_or("6379")
        );
        std::process::exit(1);
    }

    // SET benchmark
    let set_stats = run_bench(
        &addr,
        clients,
        total_requests,
        pipeline,
        |i| build_resp(&["SET", &format!("k{i}"), &format!("v{i}")]),
        |resp| resp.starts_with(b"+OK\r\n"),
    )
    .await;
    print_summary("SET", &set_stats);

    // GET benchmark — keys we just set
    let get_stats = run_bench(
        &addr,
        clients,
        total_requests,
        pipeline,
        |i| build_resp(&["GET", &format!("k{i}")]),
        |resp| resp.starts_with(b"$"),
    )
    .await;
    print_summary("GET", &get_stats);

    // INCR benchmark — single key, contention
    let _ = send_one(&addr, &build_resp(&["SET", "ctr", "0"])).await;
    let incr_stats = run_bench(
        &addr,
        clients,
        total_requests,
        pipeline,
        |_| build_resp(&["INCR", "ctr"]),
        |resp| resp.starts_with(b":"),
    )
    .await;
    print_summary("INCR", &incr_stats);

    println!();
    println!("───────────────────────────────────────────────────────────────");
    println!("These numbers go through the actual TCP stack and RESP codec,");
    println!("with backpressure, the connection-pool semaphore, and the");
    println!("dispatch path. They are directly comparable to redis-benchmark");
    println!("output. For raw storage-layer ceiling (no network, no RESP),");
    println!("see `cargo run --release --bin storage_microbench`.");
    println!();
}

fn parse_env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn build_resp(parts: &[&str]) -> Vec<u8> {
    let mut out = format!("*{}\r\n", parts.len()).into_bytes();
    for p in parts {
        out.extend_from_slice(format!("${}\r\n", p.len()).as_bytes());
        out.extend_from_slice(p.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out
}

async fn ping(addr: &str) -> bool {
    TcpStream::connect(addr).await.is_ok()
}

async fn send_one(addr: &str, frame: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut s = TcpStream::connect(addr).await?;
    s.write_all(frame).await?;
    s.flush().await?;
    let mut buf = vec![0u8; 256];
    let n = tokio::time::timeout(Duration::from_secs(5), s.read(&mut buf))
        .await
        .map_err(|_| std::io::Error::other("read timeout"))??;
    buf.truncate(n);
    Ok(buf)
}

#[derive(Debug, Clone)]
struct BenchStats {
    total_ops: u64,
    elapsed: Duration,
    latencies: Vec<Duration>,
}

#[allow(clippy::too_many_arguments)]
async fn run_bench(
    addr: &str,
    clients: usize,
    total_requests: usize,
    pipeline: usize,
    build_cmd: impl Fn(usize) -> Vec<u8> + Send + Sync + 'static,
    is_ok: impl Fn(&[u8]) -> bool + Send + Sync + 'static,
) -> BenchStats {
    let per_client = total_requests / clients.max(1);
    let actual = per_client * clients;
    let build_cmd = Arc::new(build_cmd);
    let is_ok = Arc::new(is_ok);

    let started = Instant::now();
    let mut handles = Vec::with_capacity(clients);
    for client_idx in 0..clients {
        let addr = addr.to_string();
        let build_cmd = Arc::clone(&build_cmd);
        let is_ok = Arc::clone(&is_ok);
        handles.push(tokio::spawn(async move {
            client_loop(
                &addr,
                client_idx,
                per_client,
                pipeline,
                build_cmd.as_ref(),
                is_ok.as_ref(),
            )
            .await
        }));
    }

    let mut latencies = Vec::with_capacity(actual);
    let mut ok_count = 0u64;
    for h in handles {
        match h.await {
            Ok(Ok((client_ok, mut client_latencies))) => {
                ok_count += client_ok;
                latencies.append(&mut client_latencies);
            }
            Ok(Err(e)) => eprintln!("client error: {e}"),
            Err(e) => eprintln!("join error: {e}"),
        }
    }

    BenchStats {
        total_ops: ok_count,
        elapsed: started.elapsed(),
        latencies,
    }
}

async fn client_loop(
    addr: &str,
    client_idx: usize,
    requests: usize,
    pipeline: usize,
    build_cmd: &(dyn Fn(usize) -> Vec<u8> + Send + Sync),
    is_ok: &(dyn Fn(&[u8]) -> bool + Send + Sync),
) -> std::io::Result<(u64, Vec<Duration>)> {
    let mut stream = TcpStream::connect(addr).await?;
    let mut latencies = Vec::with_capacity(requests);
    let mut ok = 0u64;

    let mut sent = 0usize;
    while sent < requests {
        let batch = pipeline.min(requests - sent);

        // Build pipelined batch.
        let mut buf = Vec::with_capacity(batch * 32);
        for j in 0..batch {
            // Use a stable key so SET k... / GET k... refer to the same
            // key across runs.
            let request_id = client_idx * requests + sent + j;
            buf.extend_from_slice(&build_cmd(request_id));
        }

        let started = Instant::now();
        stream.write_all(&buf).await?;
        stream.flush().await?;

        // Read responses for this batch. Drain as many bytes as the
        // server sends until we've seen the right number of complete
        // RESP frames.
        let mut read_buf = vec![0u8; 4096];
        let mut received = Vec::new();
        let mut frames = 0usize;
        while frames < batch {
            let n = stream.read(&mut read_buf).await?;
            if n == 0 {
                return Err(std::io::Error::other("server closed connection"));
            }
            received.extend_from_slice(&read_buf[..n]);
            // Cheap frame counter: count CRLF pairs at known terminator
            // positions. RESP simple/integer/error/bulk-header all end
            // in `\r\n`; bulk strings end in another `\r\n`. Rough but
            // adequate for the benchmark — we only need to know when the
            // server is done.
            frames = count_complete_frames(&received);
        }

        // Stamp latency for the whole batch (we measure round-trip per
        // pipeline rather than per individual request — same as
        // redis-benchmark with -P).
        let elapsed = started.elapsed();
        let per_req = elapsed / (batch as u32);
        for _ in 0..batch {
            latencies.push(per_req);
        }
        // Approximate OK count: scan the received bytes for the
        // success markers.
        if is_ok(&received) {
            ok += batch as u64;
        }

        sent += batch;
    }

    Ok((ok, latencies))
}

/// Count how many RESP frames are complete in `buf`. A frame is a
/// top-level line ending in `\r\n`, with bulk strings counting their
/// payload+CRLF before the next frame. The benchmark only needs this to
/// know when a pipelined batch's responses have all arrived.
fn count_complete_frames(buf: &[u8]) -> usize {
    let mut i = 0usize;
    let mut frames = 0usize;
    while i < buf.len() {
        let kind = buf[i];
        // Find the end of the type-line.
        let line_end = match find_crlf(buf, i + 1) {
            Some(e) => e,
            None => break,
        };
        match kind {
            b'+' | b'-' | b':' => {
                i = line_end + 2;
                frames += 1;
            }
            b'$' => {
                let len_str = std::str::from_utf8(&buf[i + 1..line_end]).unwrap_or("");
                let len: i32 = len_str.parse().unwrap_or(-1);
                if len < 0 {
                    // nil bulk
                    i = line_end + 2;
                    frames += 1;
                } else {
                    let payload_end = line_end + 2 + len as usize;
                    if payload_end + 2 > buf.len() {
                        break;
                    }
                    i = payload_end + 2;
                    frames += 1;
                }
            }
            b'*' => {
                // Top-level arrays — for benchmark commands we only
                // expect simple/bulk/integer responses. If we see an
                // array, give up frame-counting and break (caller will
                // re-read).
                break;
            }
            _ => break,
        }
    }
    frames
}

fn find_crlf(buf: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    while i + 1 < buf.len() {
        if buf[i] == b'\r' && buf[i + 1] == b'\n' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn print_summary(name: &str, stats: &BenchStats) {
    let mut sorted = stats.latencies.clone();
    sorted.sort_unstable();

    let total = stats.total_ops;
    let secs = stats.elapsed.as_secs_f64();
    let ops_per_sec = if secs > 0.0 { total as f64 / secs } else { 0.0 };

    let pct = |p: f64| -> Duration {
        if sorted.is_empty() {
            Duration::ZERO
        } else {
            let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
            sorted[idx.min(sorted.len() - 1)]
        }
    };

    println!();
    println!("─── {name} ─────────────────────────────────────────────────");
    println!(
        "  throughput:    {:>10.0} ops/sec     (over {:.2}s)",
        ops_per_sec, secs
    );
    println!(
        "  successful:    {:>10}            (target {})",
        total,
        sorted.len()
    );
    println!("  latency p50:   {:>10}", fmt_dur(pct(0.50)));
    println!("  latency p95:   {:>10}", fmt_dur(pct(0.95)));
    println!("  latency p99:   {:>10}", fmt_dur(pct(0.99)));
    println!("  latency max:   {:>10}", fmt_dur(pct(1.0)));
}

fn fmt_dur(d: Duration) -> String {
    let nanos = d.as_nanos();
    if nanos < 1_000 {
        format!("{nanos} ns")
    } else if nanos < 1_000_000 {
        format!("{:.2} µs", nanos as f64 / 1_000.0)
    } else if nanos < 1_000_000_000 {
        format!("{:.2} ms", nanos as f64 / 1_000_000.0)
    } else {
        format!("{:.2} s", nanos as f64 / 1_000_000_000.0)
    }
}
