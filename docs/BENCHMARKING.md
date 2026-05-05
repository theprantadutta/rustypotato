# Benchmarking

There are three benchmarks in this repository, and they measure
different things. Reading just one number out of context will mislead
you, so this page is honest about which is which and when each one is
the right tool.

## TL;DR

| Tool | What it measures | When to use |
|---|---|---|
| `tools/network_bench.rs` | RESP-over-TCP, real client, full round-trip | Numbers you can compare against `redis-benchmark` or other key-value stores |
| `cargo bench` (Criterion) | Statistical micro-benchmarks of in-process operations | Catching regressions in hot paths during development |
| `tools/storage_microbench.rs` | `MemoryStore` operations directly, no protocol | Storage-layer ceiling; reflects "how fast could this go if the network were free" |

If you just want a number to show someone, **always run
`network_bench`.** The other two are diagnostic tools, not headline
performance claims.

---

## Network benchmark — the honest one

`tools/network_bench.rs` connects to a running server over TCP, encodes
commands as RESP, runs N concurrent clients with a configurable
pipeline depth, and counts round-trip operations per second. These are
the numbers a real client sees and they are directly comparable to
`redis-benchmark` output.

### Run it

```bash
# Build the release binary first
cargo build --release

# Start the server
./target/release/rustypotato-server --port 6379 &

# Run the benchmark (defaults: 50 clients, 100k requests, pipeline=16)
cargo run --release --bin network_bench
```

Output reports throughput and p50/p95/p99 latency for SET, GET, and
INCR.

### Tweak it

Environment variables, all optional:

| Variable | Default | Description |
|---|---|---|
| `BENCH_ADDR` | `127.0.0.1:6379` | Server address |
| `BENCH_CLIENTS` | `50` | Number of concurrent clients |
| `BENCH_REQUESTS` | `100000` | Total requests across all clients |
| `BENCH_PIPELINE` | `16` | Pipeline depth (commands per round-trip) |

```bash
BENCH_CLIENTS=100 BENCH_PIPELINE=32 cargo run --release --bin network_bench
```

### Comparing against Redis

Run both servers on the same hardware with the same workload:

```bash
# Against rustypotato
./target/release/rustypotato-server --port 6379 &
cargo run --release --bin network_bench

# Against real Redis
redis-server --port 6379 &
cargo run --release --bin network_bench
```

Or use `redis-benchmark` (from the Redis distribution) directly against
either:

```bash
redis-benchmark -h 127.0.0.1 -p 6379 -n 100000 -c 50 -t set,get,incr
```

---

## Criterion micro-benchmarks

`benches/performance_benchmarks.rs` runs detailed in-process benchmarks
with statistical analysis (warm-up, outlier detection, confidence
intervals). It's the right tool for catching regressions during
development — slower than the network bench but with much tighter
measurement.

```bash
# Run all groups
cargo bench --bench performance_benchmarks

# Run a specific group
cargo bench --bench performance_benchmarks -- memory_store
cargo bench --bench performance_benchmarks -- integer_operations
cargo bench --bench performance_benchmarks -- ttl_operations
cargo bench --bench performance_benchmarks -- network_throughput
cargo bench --bench performance_benchmarks -- scalability
cargo bench --bench performance_benchmarks -- memory_patterns

# HTML report
open target/criterion/report/index.html
```

Groups:

- **memory_store** — core key-value ops directly on `MemoryStore`
- **integer_operations** — INCR / DECR latencies
- **ttl_operations** — EXPIRE / TTL
- **network_throughput** — full TCP round-trip with various payload sizes
- **scalability** — GET latency at 1K, 10K, 100K keys in store
- **memory_patterns** — buffer reuse, Arc cloning, allocation costs

---

## Storage microbench — the ceiling

`tools/storage_microbench.rs` calls `MemoryStore` directly, with no TCP,
no RESP, no connection pool, no command dispatch. It exists so we can
catch regressions in the hot read path in isolation, but it is **not**
a "what does this server do for clients" claim.

```bash
cargo run --release --bin storage_microbench
```

The output is intentionally banner-loud about not being a network
benchmark. If you find yourself quoting these numbers, run
`network_bench` instead and quote those.

---

## Why no quoted numbers in this doc

Earlier versions of this repository quoted in-process microbench
numbers (GET ~1.1M ops/sec, etc.) as headline performance — which was
misleading because those bypass the entire network and protocol layer.

Quoted numbers also rot fast: a benchmark on a 2018 laptop is almost
useless to someone running on 2024 server hardware, and a benchmark on
default tokio worker counts is useless to someone running with
`worker_threads=N` tuned for their CPU.

So the policy is: **run the benchmark on your own hardware, with your
own workload, and quote those numbers.** That way the numbers actually
mean something for your deployment.

---

## Performance regression tests

There's a small set of threshold tests in `tests/performance_tests.rs`
that assert "GET completes in under N ms" etc. They're marked
`#[ignore]` so they don't run on every `cargo test`:

```bash
cargo test --test performance_tests -- --ignored
```

These tests are intentionally generous — they catch catastrophic
regressions (e.g. GET suddenly taking milliseconds), not subtle
regressions (e.g. GET going from 1 µs to 1.1 µs). Use Criterion for
that.

---

## Troubleshooting

### "Address in use"

Another process is on port 6379:

```bash
# Linux / Mac
lsof -i :6379
kill -9 <pid>

# Windows
netstat -ano | findstr :6379
taskkill /PID <pid> /F
```

### Inconsistent results

- Close background applications (browsers, IDEs, etc. — they steal CPU).
- Run benchmarks multiple times and look for stability, not a single number.
- For Criterion, `cargo bench -- --warm-up-time 5` increases warm-up time.
- For `network_bench`, increase `BENCH_REQUESTS` to amortize startup costs.

### Benchmark too slow

Run a specific Criterion group instead of all of them:

```bash
cargo bench --bench performance_benchmarks -- memory_store
```

For `network_bench`, drop `BENCH_REQUESTS` while iterating:

```bash
BENCH_REQUESTS=10000 cargo run --release --bin network_bench
```
