//! Performance benchmarks for RustyPotato
//!
//! This module contains comprehensive benchmarks for measuring throughput,
//! latency, and performance characteristics of RustyPotato operations.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rustypotato::{Config, MemoryStore, MetricsCollector, RustyPotatoServer, ValueType};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::runtime::Runtime;

/// Benchmark memory store operations
fn bench_memory_store(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let store = rt.block_on(async { Arc::new(MemoryStore::new()) });

    let mut group = c.benchmark_group("memory_store");

    // Benchmark SET operations
    group.bench_function("set_string", |b| {
        let store = Arc::clone(&store);
        b.to_async(&rt).iter(|| async {
            let key = format!("key_{}", fastrand::u64(..));
            let value = ValueType::String(format!("value_{}", fastrand::u64(..)));
            store.set(black_box(key), black_box(value)).await.unwrap();
        });
    });

    // Benchmark GET operations
    group.bench_function("get_existing", |b| {
        let store = Arc::clone(&store);
        // Pre-populate with test data
        rt.block_on(async {
            for i in 0..1000 {
                let key = format!("bench_key_{i}");
                let value = ValueType::String(format!("bench_value_{i}"));
                store.set(key, value).await.unwrap();
            }
        });

        b.to_async(&rt).iter(|| async {
            let key = format!("bench_key_{}", fastrand::usize(..1000));
            let _ = store.get(black_box(&key)).unwrap();
        });
    });

    // Benchmark GET operations for non-existent keys
    group.bench_function("get_missing", |b| {
        let store = Arc::clone(&store);
        b.to_async(&rt).iter(|| async {
            let key = format!("missing_key_{}", fastrand::u64(..));
            let _ = store.get(black_box(&key)).unwrap();
        });
    });

    // Benchmark DELETE operations
    group.bench_function("delete", |b| {
        let store = Arc::clone(&store);
        b.to_async(&rt).iter(|| async {
            // Setup and execute in the same async block
            let key = format!("delete_key_{}", fastrand::u64(..));
            let value = ValueType::String("delete_value".to_string());
            store.set(key.clone(), value).await.unwrap();
            store.delete(black_box(&key)).await.unwrap();
        });
    });

    // Benchmark concurrent operations
    group.bench_function("concurrent_mixed", |b| {
        let store = Arc::clone(&store);
        b.to_async(&rt).iter(|| async {
            let tasks = (0..10).map(|i| {
                let store = Arc::clone(&store);
                tokio::spawn(async move {
                    match i % 3 {
                        0 => {
                            // SET operation
                            let key = format!("concurrent_key_{}", fastrand::u64(..));
                            let value = ValueType::String(format!("value_{}", fastrand::u64(..)));
                            store.set(key, value).await.unwrap();
                        }
                        1 => {
                            // GET operation
                            let key = format!("concurrent_key_{}", fastrand::u64(..));
                            let _ = store.get(&key).unwrap();
                        }
                        _ => {
                            // DELETE operation
                            let key = format!("concurrent_key_{}", fastrand::u64(..));
                            let _ = store.delete(&key).await.unwrap();
                        }
                    }
                })
            });

            futures::future::join_all(tasks).await;
        });
    });

    group.finish();
}

/// Benchmark integer operations
fn bench_integer_operations(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let store = rt.block_on(async { Arc::new(MemoryStore::new()) });

    let mut group = c.benchmark_group("integer_operations");

    // Benchmark INCR operations
    group.bench_function("incr_new_key", |b| {
        let store = Arc::clone(&store);
        b.to_async(&rt).iter(|| async {
            let key = format!("incr_key_{}", fastrand::u64(..));
            store.incr(black_box(&key)).await.unwrap();
        });
    });

    // Benchmark INCR on existing keys
    group.bench_function("incr_existing_key", |b| {
        let store = Arc::clone(&store);
        // Pre-populate with integer values
        rt.block_on(async {
            for i in 0..1000 {
                let key = format!("incr_existing_{i}");
                store.set(key, ValueType::Integer(0)).await.unwrap();
            }
        });

        b.to_async(&rt).iter(|| async {
            let key = format!("incr_existing_{}", fastrand::usize(..1000));
            store.incr(black_box(&key)).await.unwrap();
        });
    });

    // Benchmark DECR operations
    group.bench_function("decr_existing_key", |b| {
        let store = Arc::clone(&store);
        // Pre-populate with integer values
        rt.block_on(async {
            for i in 0..1000 {
                let key = format!("decr_existing_{i}");
                store.set(key, ValueType::Integer(100)).await.unwrap();
            }
        });

        b.to_async(&rt).iter(|| async {
            let key = format!("decr_existing_{}", fastrand::usize(..1000));
            store.decr(black_box(&key)).await.unwrap();
        });
    });

    group.finish();
}

/// Benchmark TTL operations
fn bench_ttl_operations(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let store = rt.block_on(async { Arc::new(MemoryStore::new()) });

    let mut group = c.benchmark_group("ttl_operations");

    // Benchmark EXPIRE operations
    group.bench_function("expire", |b| {
        let store = Arc::clone(&store);
        b.to_async(&rt).iter(|| async {
            // Setup and execute in the same async block
            let key = format!("expire_key_{}", fastrand::u64(..));
            let value = ValueType::String("expire_value".to_string());
            store.set(key.clone(), value).await.unwrap();
            store.expire(black_box(&key), 60).await.unwrap();
        });
    });

    // Benchmark TTL operations
    group.bench_function("ttl", |b| {
        let store = Arc::clone(&store);
        // Pre-populate with keys that have TTL
        rt.block_on(async {
            for i in 0..1000 {
                let key = format!("ttl_key_{i}");
                let value = ValueType::String(format!("ttl_value_{i}"));
                store.set(key.clone(), value).await.unwrap();
                store.expire(&key, 3600).await.unwrap();
            }
        });

        b.to_async(&rt).iter(|| async {
            let key = format!("ttl_key_{}", fastrand::usize(..1000));
            store.ttl(black_box(&key)).unwrap();
        });
    });

    group.finish();
}

/// Benchmark network throughput
fn bench_network_throughput(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let mut group = c.benchmark_group("network_throughput");
    group.throughput(Throughput::Elements(1));

    // Test different payload sizes
    for size in [64, 256, 1024, 4096, 16384].iter() {
        group.bench_with_input(BenchmarkId::new("tcp_echo", size), size, |b, &size| {
            // Pre-setup the echo server outside the benchmark
            let addr = rt.block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let addr = listener.local_addr().unwrap();

                // Spawn echo server
                tokio::spawn(async move {
                    while let Ok((mut stream, _)) = listener.accept().await {
                        tokio::spawn(async move {
                            let mut buffer = vec![0; 65536];
                            while let Ok(n) = stream.read(&mut buffer).await {
                                if n == 0 {
                                    break;
                                }
                                let _ = stream.write_all(&buffer[..n]).await;
                            }
                        });
                    }
                });

                addr
            });

            b.to_async(&rt).iter(|| async move {
                let mut stream = TcpStream::connect(addr).await.unwrap();
                let data = vec![b'x'; size];
                let mut response = vec![0; size];

                stream.write_all(black_box(&data)).await.unwrap();
                stream.read_exact(&mut response).await.unwrap();

                assert_eq!(data, response);
            });
        });
    }

    group.finish();
}

/// Benchmark metrics collection overhead
fn bench_metrics_collection(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let metrics = rt.block_on(async { Arc::new(MetricsCollector::new()) });

    let mut group = c.benchmark_group("metrics_collection");

    // Benchmark recording command latency
    group.bench_function("record_command_latency", |b| {
        let metrics = Arc::clone(&metrics);
        b.to_async(&rt).iter(|| async {
            metrics
                .record_command_latency(black_box("SET"), black_box(Duration::from_micros(100)))
                .await;
        });
    });

    // Benchmark recording network bytes
    group.bench_function("record_network_bytes", |b| {
        let metrics = Arc::clone(&metrics);
        b.to_async(&rt).iter(|| async {
            metrics
                .record_network_bytes(black_box(1024), black_box(512))
                .await;
        });
    });

    // Benchmark getting metrics summary
    group.bench_function("get_summary", |b| {
        let metrics = Arc::clone(&metrics);
        b.to_async(&rt).iter(|| async {
            metrics.get_summary().await;
        });
    });

    group.finish();
}

/// Benchmark full server operations
fn bench_server_operations(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let mut group = c.benchmark_group("server_operations");
    group.sample_size(10); // Fewer samples for expensive operations
    group.measurement_time(Duration::from_secs(30));

    // Benchmark server startup and shutdown
    group.bench_function("server_lifecycle", |b| {
        b.to_async(&rt).iter(|| async {
            let mut config = Config::default();
            config.server.port = 0; // Use random port
            config.storage.aof_enabled = false; // Disable persistence for benchmarking

            let mut server = RustyPotatoServer::new(config).unwrap();
            let addr = server.start_with_addr().await.unwrap();

            // Simulate some client activity
            let mut stream = TcpStream::connect(addr).await.unwrap();
            let command = b"*3\r\n$3\r\nSET\r\n$4\r\ntest\r\n$5\r\nvalue\r\n";
            stream.write_all(command).await.unwrap();

            let mut response = [0; 1024];
            let _ = stream.read(&mut response).await.unwrap();

            // Server cleanup happens when it goes out of scope
        });
    });

    group.finish();
}

/// Benchmark data structure scalability
fn bench_scalability(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let mut group = c.benchmark_group("scalability");
    group.sample_size(10);

    // Test performance with different numbers of keys
    for key_count in [1_000, 10_000, 100_000].iter() {
        group.bench_with_input(
            BenchmarkId::new("get_with_keys", key_count),
            key_count,
            |b, &key_count| {
                let store = rt.block_on(async {
                    let store = Arc::new(MemoryStore::new());

                    // Pre-populate with keys
                    for i in 0..key_count {
                        let key = format!("scale_key_{i}");
                        let value = ValueType::String(format!("scale_value_{i}"));
                        store.set(key, value).await.unwrap();
                    }

                    store
                });

                b.to_async(&rt).iter(|| async {
                    let key = format!("scale_key_{}", fastrand::usize(..key_count));
                    store.get(black_box(&key)).unwrap();
                });
            },
        );
    }

    group.finish();
}

/// Benchmark memory allocation patterns
fn bench_memory_patterns(c: &mut Criterion) {
    let _rt = Runtime::new().unwrap();

    let mut group = c.benchmark_group("memory_patterns");

    // Benchmark string allocation patterns
    group.bench_function("string_allocation", |b| {
        b.iter(|| {
            let key = format!("key_{}", black_box(fastrand::u64(..)));
            let value = format!("value_{}", black_box(fastrand::u64(..)));
            (key, value)
        });
    });

    // Benchmark buffer reuse patterns
    group.bench_function("buffer_reuse", |b| {
        let mut buffer = Vec::with_capacity(1024);
        b.iter(|| {
            buffer.clear();
            buffer.extend_from_slice(black_box(b"test data for buffer reuse benchmark"));
            buffer.len()
        });
    });

    // Benchmark Arc cloning overhead
    group.bench_function("arc_clone", |b| {
        let data = Arc::new(vec![1u8; 1024]);
        b.iter(|| {
            let _cloned = Arc::clone(black_box(&data));
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_memory_store,
    bench_integer_operations,
    bench_ttl_operations,
    bench_network_throughput,
    bench_metrics_collection,
    bench_server_operations,
    bench_scalability,
    bench_memory_patterns
);

criterion_main!(benches);
