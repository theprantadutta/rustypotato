# RustyPotato Performance Benchmarking Guide

This document explains how to measure RustyPotato's performance and what results to expect.

## Quick Start

```bash
# Run all benchmarks (detailed Criterion reports)
cargo bench --bench performance_benchmarks

# Run quick performance check (human-readable summary)
cargo run --release --bin quick_bench

# View HTML reports (after running cargo bench)
# Open: target/criterion/report/index.html
```

## Performance Results Summary

Benchmarks run on this system show the following performance characteristics:

### Core Operations (In-Memory Store)

| Operation | Latency | Throughput |
|-----------|---------|------------|
| **SET** | ~5.2 µs | ~194,000 ops/sec |
| **GET** (existing key) | ~970 ns | ~1,032,000 ops/sec |
| **GET** (missing key) | ~740 ns | ~1,355,000 ops/sec |
| **DELETE** | ~2.4 µs | ~415,000 ops/sec |

### Integer Operations

| Operation | Latency | Throughput |
|-----------|---------|------------|
| **INCR** (new key) | ~3.6 µs | ~280,000 ops/sec |
| **INCR** (existing key) | ~1.4 µs | ~698,000 ops/sec |
| **DECR** (existing key) | ~1.5 µs | ~662,000 ops/sec |

### TTL Operations

| Operation | Latency | Throughput |
|-----------|---------|------------|
| **EXPIRE** | ~4.7 µs | ~213,000 ops/sec |
| **TTL** | ~750 ns | ~1,335,000 ops/sec |

### Network Throughput (Full TCP Round-Trip)

| Payload Size | Latency | Throughput |
|--------------|---------|------------|
| 64 bytes | ~1.3 ms | ~780 ops/sec |
| 256 bytes | ~1.5 ms | ~650 ops/sec |
| 1 KB | ~4.9 ms | ~204 ops/sec |
| 4 KB | ~5.7 ms | ~175 ops/sec |
| 16 KB | ~5.7 ms | ~176 ops/sec |

### Metrics Collection Overhead

| Operation | Overhead |
|-----------|----------|
| Record command latency | ~555 ns |
| Record network bytes | ~87 ns |
| Get metrics summary | ~4.5 µs |

## Running Benchmarks

### Prerequisites

- Rust 1.70+ (with `cargo bench` support)
- At least 4GB RAM recommended
- No other CPU-intensive processes running

### Full Criterion Benchmarks

```bash
# Run all benchmarks with detailed statistics
cargo bench --bench performance_benchmarks

# Run specific benchmark group
cargo bench --bench performance_benchmarks -- memory_store
cargo bench --bench performance_benchmarks -- integer_operations
cargo bench --bench performance_benchmarks -- ttl_operations
cargo bench --bench performance_benchmarks -- network_throughput
cargo bench --bench performance_benchmarks -- metrics_collection
cargo bench --bench performance_benchmarks -- scalability
cargo bench --bench performance_benchmarks -- memory_patterns
```

### Quick Benchmark Tool

For a simple, human-readable performance report:

```bash
cargo run --release --bin quick_bench
```

This outputs:
```
RustyPotato Performance Report
==============================
SET operations:  194,000 ops/sec
GET operations:  1,032,000 ops/sec
Latency (p50):   ~1 µs
Latency (p99):   ~10 µs
```

### Performance Regression Tests

```bash
# Run performance threshold tests
cargo test --test performance_tests -- --ignored
```

These tests verify:
- SET latency < 1ms
- GET latency < 0.5ms
- Throughput > 10,000 ops/sec

## Interpreting Results

### What the Numbers Mean

- **Latency**: Time for a single operation to complete
- **Throughput**: Operations per second (ops/sec)
- **µs (microseconds)**: 1/1,000,000 of a second
- **ns (nanoseconds)**: 1/1,000,000,000 of a second

### Memory Store vs Network Performance

The memory store benchmarks measure **pure in-memory performance** without network overhead:
- GET operations exceed 1 million ops/sec
- These represent the theoretical maximum

Network benchmarks include **full TCP round-trip**:
- Connect, send command, receive response
- Latency is dominated by TCP/IP stack overhead
- Real-world performance with actual Redis clients

### Factors Affecting Performance

1. **CPU Speed**: Higher clock speeds improve single-threaded operations
2. **Memory Bandwidth**: Affects large value operations
3. **Network Latency**: Dominates client-server performance
4. **Concurrent Connections**: DashMap provides lock-free scalability
5. **Value Size**: Larger values increase serialization overhead

## Comparison Methodology

To compare with other key-value stores:

### Fair Comparison Setup

1. Use the same hardware
2. Disable persistence (AOF) for pure memory comparison
3. Use single client for latency tests
4. Use multiple clients for throughput tests
5. Test with similar value sizes (default: small strings)

### Benchmarking Against Redis

```bash
# Start RustyPotato
cargo run --release --bin server

# Use redis-benchmark (from Redis installation)
redis-benchmark -h 127.0.0.1 -p 6379 -t set,get -n 100000 -q
```

## Performance Targets

RustyPotato aims to meet these targets:

| Metric | Target | Status |
|--------|--------|--------|
| GET latency | < 500 µs | ✅ ~1 µs |
| SET latency | < 1 ms | ✅ ~5 µs |
| Throughput | > 10K ops/sec | ✅ > 100K ops/sec |
| Memory overhead | < 2x value size | ✅ |
| Startup time | < 1 second | ✅ |

## Benchmark Groups

### memory_store
Tests core key-value operations directly on MemoryStore:
- `set_string`: Store a string value
- `get_existing`: Retrieve existing key
- `get_missing`: Query non-existent key
- `delete`: Remove a key
- `concurrent_mixed`: Parallel SET/GET operations

### integer_operations
Tests atomic counter operations:
- `incr_new_key`: INCR on new key
- `incr_existing_key`: INCR on existing counter
- `decr_existing_key`: DECR operation

### ttl_operations
Tests expiration functionality:
- `expire`: Set TTL on key
- `ttl`: Check remaining TTL

### network_throughput
Tests full TCP round-trip with various payload sizes:
- 64B, 256B, 1KB, 4KB, 16KB payloads

### scalability
Tests performance at different data sizes:
- GET with 1K, 10K, 100K keys in store

### memory_patterns
Tests memory allocation efficiency:
- Buffer reuse
- Arc cloning overhead
- Allocation patterns

## Generating Reports

After running `cargo bench`, find HTML reports at:

```
target/criterion/report/index.html
```

These include:
- Detailed statistical analysis
- Performance over time (if run multiple times)
- Outlier detection
- Confidence intervals

## Troubleshooting

### "Address in use" errors
Another process is using the default port. Kill existing instances:
```bash
# Windows
netstat -ano | findstr :6379
taskkill /PID <pid> /F

# Linux/Mac
lsof -i :6379
kill -9 <pid>
```

### Inconsistent results
- Close background applications
- Run benchmarks multiple times
- Use `cargo bench -- --warm-up-time 5` for more stable results

### Benchmark takes too long
Run specific groups:
```bash
cargo bench --bench performance_benchmarks -- memory_store
```
