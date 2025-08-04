//! Performance regression tests with benchmarking
//!
//! These tests verify that performance characteristics remain within
//! acceptable bounds and detect performance regressions.

use rustypotato::{Config, RustyPotatoServer, MemoryStore, ValueType};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};

/// Performance thresholds for regression detection
const MAX_SET_LATENCY_MICROS: u64 = 1000; // 1ms
const MAX_GET_LATENCY_MICROS: u64 = 500;  // 0.5ms
const MAX_INCR_LATENCY_MICROS: u64 = 1000; // 1ms
const MIN_THROUGHPUT_OPS_PER_SEC: u64 = 10000; // 10K ops/sec
const MAX_MEMORY_OVERHEAD_BYTES_PER_KEY: usize = 200; // 200 bytes per key

/// Helper function to send RESP command and measure latency
async fn send_command_with_timing(stream: &mut TcpStream, command: &[u8]) -> Result<(Vec<u8>, Duration), Box<dyn std::error::Error>> {
    let start = Instant::now();
    
    stream.write_all(command).await?;
    stream.flush().await?;
    
    let mut buffer = vec![0u8; 4096];
    let n = timeout(Duration::from_secs(10), stream.read(&mut buffer)).await??;
    buffer.truncate(n);
    
    let elapsed = start.elapsed();
    Ok((buffer, elapsed))
}

/// Create a test server for performance testing
async fn create_performance_test_server() -> (RustyPotatoServer, std::net::SocketAddr) {
    let mut config = Config::default();
    config.server.port = 0;
    config.storage.aof_enabled = false; // Disable for pure performance testing
    config.server.max_connections = 10000;
    
    let mut server = RustyPotatoServer::new(config).unwrap();
    let addr = server.start_with_addr().await.unwrap();
    
    sleep(Duration::from_millis(100)).await;
    
    (server, addr)
}

#[tokio::test]
async fn test_single_operation_latency_regression() {
    let (_server, addr) = create_performance_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();
    
    // Warm up
    for i in 0..100 {
        let key = format!("warmup_{}", i);
        let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n$5\r\nvalue\r\n", key.len(), key);
        let _ = send_command_with_timing(&mut stream, set_cmd.as_bytes()).await.unwrap();
    }
    
    // Test SET operation latency
    let mut set_latencies = Vec::new();
    for i in 0..1000 {
        let key = format!("latency_test_{}", i);
        let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n$5\r\nvalue\r\n", key.len(), key);
        let (response, latency) = send_command_with_timing(&mut stream, set_cmd.as_bytes()).await.unwrap();
        assert_eq!(response, b"+OK\r\n");
        set_latencies.push(latency);
    }
    
    // Test GET operation latency
    let mut get_latencies = Vec::new();
    for i in 0..1000 {
        let key = format!("latency_test_{}", i);
        let get_cmd = format!("*2\r\n$3\r\nGET\r\n${}\r\n{}\r\n", key.len(), key);
        let (response, latency) = send_command_with_timing(&mut stream, get_cmd.as_bytes()).await.unwrap();
        assert_eq!(response, b"$5\r\nvalue\r\n");
        get_latencies.push(latency);
    }
    
    // Test INCR operation latency
    let mut incr_latencies = Vec::new();
    for i in 0..1000 {
        let counter = format!("counter_{}", i % 10); // Use 10 different counters
        let incr_cmd = format!("*2\r\n$4\r\nINCR\r\n${}\r\n{}\r\n", counter.len(), counter);
        let (_response, latency) = send_command_with_timing(&mut stream, incr_cmd.as_bytes()).await.unwrap();
        incr_latencies.push(latency);
    }
    
    // Calculate statistics
    let avg_set_latency = set_latencies.iter().sum::<Duration>() / set_latencies.len() as u32;
    let max_set_latency = *set_latencies.iter().max().unwrap();
    let p99_set_latency = {
        let mut sorted = set_latencies.clone();
        sorted.sort();
        sorted[(sorted.len() * 99 / 100).min(sorted.len() - 1)]
    };
    
    let avg_get_latency = get_latencies.iter().sum::<Duration>() / get_latencies.len() as u32;
    let max_get_latency = *get_latencies.iter().max().unwrap();
    let p99_get_latency = {
        let mut sorted = get_latencies.clone();
        sorted.sort();
        sorted[(sorted.len() * 99 / 100).min(sorted.len() - 1)]
    };
    
    let avg_incr_latency = incr_latencies.iter().sum::<Duration>() / incr_latencies.len() as u32;
    let max_incr_latency = *incr_latencies.iter().max().unwrap();
    let p99_incr_latency = {
        let mut sorted = incr_latencies.clone();
        sorted.sort();
        sorted[(sorted.len() * 99 / 100).min(sorted.len() - 1)]
    };
    
    // Print performance statistics
    println!("SET - Avg: {:?}, Max: {:?}, P99: {:?}", avg_set_latency, max_set_latency, p99_set_latency);
    println!("GET - Avg: {:?}, Max: {:?}, P99: {:?}", avg_get_latency, max_get_latency, p99_get_latency);
    println!("INCR - Avg: {:?}, Max: {:?}, P99: {:?}", avg_incr_latency, max_incr_latency, p99_incr_latency);
    
    // Assert performance thresholds
    assert!(p99_set_latency.as_micros() < MAX_SET_LATENCY_MICROS as u128, 
           "SET P99 latency regression: {:?} >= {}μs", p99_set_latency, MAX_SET_LATENCY_MICROS);
    
    assert!(p99_get_latency.as_micros() < MAX_GET_LATENCY_MICROS as u128,
           "GET P99 latency regression: {:?} >= {}μs", p99_get_latency, MAX_GET_LATENCY_MICROS);
    
    assert!(p99_incr_latency.as_micros() < MAX_INCR_LATENCY_MICROS as u128,
           "INCR P99 latency regression: {:?} >= {}μs", p99_incr_latency, MAX_INCR_LATENCY_MICROS);
}

#[tokio::test]
async fn test_throughput_regression() {
    let (_server, addr) = create_performance_test_server().await;
    
    let operation_count = 10000;
    let start_time = Instant::now();
    
    // Perform operations as fast as possible
    let mut stream = TcpStream::connect(addr).await.unwrap();
    
    for i in 0..operation_count {
        let key = format!("throughput_{}", i);
        let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n$5\r\nvalue\r\n", key.len(), key);
        
        stream.write_all(set_cmd.as_bytes()).await.unwrap();
        stream.flush().await.unwrap();
        
        let mut buffer = vec![0u8; 1024];
        let n = stream.read(&mut buffer).await.unwrap();
        buffer.truncate(n);
        assert_eq!(buffer, b"+OK\r\n");
    }
    
    let elapsed = start_time.elapsed();
    let ops_per_sec = (operation_count as f64 / elapsed.as_secs_f64()) as u64;
    
    println!("Throughput: {} ops/sec", ops_per_sec);
    
    assert!(ops_per_sec >= MIN_THROUGHPUT_OPS_PER_SEC,
           "Throughput regression: {} ops/sec < {} ops/sec", ops_per_sec, MIN_THROUGHPUT_OPS_PER_SEC);
}

#[tokio::test]
async fn test_concurrent_throughput_regression() {
    let (_server, addr) = create_performance_test_server().await;
    
    let client_count = 10;
    let operations_per_client = 1000;
    let start_time = Instant::now();
    
    let mut handles = Vec::new();
    
    // Spawn multiple concurrent clients
    for client_id in 0..client_count {
        let handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            
            for i in 0..operations_per_client {
                let key = format!("concurrent_{}_{}", client_id, i);
                let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n$5\r\nvalue\r\n", key.len(), key);
                
                stream.write_all(set_cmd.as_bytes()).await.unwrap();
                stream.flush().await.unwrap();
                
                let mut buffer = vec![0u8; 1024];
                let n = stream.read(&mut buffer).await.unwrap();
                buffer.truncate(n);
                assert_eq!(buffer, b"+OK\r\n");
            }
            
            client_id
        });
        handles.push(handle);
    }
    
    // Wait for all clients to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    let elapsed = start_time.elapsed();
    let total_operations = client_count * operations_per_client;
    let ops_per_sec = (total_operations as f64 / elapsed.as_secs_f64()) as u64;
    
    println!("Concurrent throughput: {} ops/sec with {} clients", ops_per_sec, client_count);
    
    // Concurrent throughput should be higher than single-threaded
    let min_concurrent_throughput = MIN_THROUGHPUT_OPS_PER_SEC * 2; // Expect at least 2x improvement
    assert!(ops_per_sec >= min_concurrent_throughput,
           "Concurrent throughput regression: {} ops/sec < {} ops/sec", ops_per_sec, min_concurrent_throughput);
}

#[tokio::test]
async fn test_memory_usage_regression() {
    let store = Arc::new(MemoryStore::new());
    let key_count = 10000;
    
    // Measure initial memory stats
    let initial_stats = store.memory_stats();
    
    // Add many keys
    for i in 0..key_count {
        let key = format!("memory_test_{:06}", i);
        let value = format!("value_{:06}", i);
        store.set(&key, value.clone()).await.unwrap();
    }
    
    // Measure final memory stats
    let final_stats = store.memory_stats();
    
    assert_eq!(final_stats.key_count, key_count);
    
    // Calculate memory overhead per key
    let memory_used = final_stats.estimated_memory_bytes - initial_stats.estimated_memory_bytes;
    let memory_per_key = memory_used / key_count;
    
    println!("Memory usage: {} bytes total, {} bytes per key", memory_used, memory_per_key);
    
    assert!(memory_per_key <= MAX_MEMORY_OVERHEAD_BYTES_PER_KEY,
           "Memory usage regression: {} bytes per key > {} bytes per key", 
           memory_per_key, MAX_MEMORY_OVERHEAD_BYTES_PER_KEY);
}

#[tokio::test]
async fn test_large_value_performance() {
    let (_server, addr) = create_performance_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();
    
    // Test with different value sizes
    let value_sizes = [1024, 4096, 16384, 65536]; // 1KB, 4KB, 16KB, 64KB
    
    for &size in &value_sizes {
        let large_value = "x".repeat(size);
        let key = format!("large_value_{}", size);
        
        // Measure SET latency
        let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n", 
                             key.len(), key, large_value.len(), large_value);
        let start = Instant::now();
        
        stream.write_all(set_cmd.as_bytes()).await.unwrap();
        stream.flush().await.unwrap();
        
        let mut buffer = vec![0u8; 1024];
        let n = stream.read(&mut buffer).await.unwrap();
        buffer.truncate(n);
        assert_eq!(buffer, b"+OK\r\n");
        
        let set_latency = start.elapsed();
        
        // Measure GET latency
        let get_cmd = format!("*2\r\n$3\r\nGET\r\n${}\r\n{}\r\n", key.len(), key);
        let start = Instant::now();
        
        stream.write_all(get_cmd.as_bytes()).await.unwrap();
        stream.flush().await.unwrap();
        
        // Read response with larger buffer for large values
        let mut buffer = vec![0u8; size + 1024];
        let n = stream.read(&mut buffer).await.unwrap();
        buffer.truncate(n);
        
        let get_latency = start.elapsed();
        
        println!("Size: {} bytes, SET: {:?}, GET: {:?}", size, set_latency, get_latency);
        
        // Performance should scale reasonably with size
        let max_latency_per_kb = Duration::from_micros(100); // 100μs per KB
        let size_kb = size / 1024;
        let max_expected_latency = max_latency_per_kb * size_kb as u32;
        
        assert!(set_latency <= max_expected_latency,
               "Large value SET latency regression: {:?} > {:?} for {} bytes", 
               set_latency, max_expected_latency, size);
        
        assert!(get_latency <= max_expected_latency,
               "Large value GET latency regression: {:?} > {:?} for {} bytes", 
               get_latency, max_expected_latency, size);
    }
}

#[tokio::test]
async fn test_atomic_operations_performance() {
    let (_server, addr) = create_performance_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();
    
    let operation_count = 5000;
    let start_time = Instant::now();
    
    // Perform many atomic increments on the same counter
    for _ in 0..operation_count {
        let incr_cmd = b"*2\r\n$4\r\nINCR\r\n$15\r\natomic_counter\r\n";
        
        stream.write_all(incr_cmd).await.unwrap();
        stream.flush().await.unwrap();
        
        let mut buffer = vec![0u8; 1024];
        let n = stream.read(&mut buffer).await.unwrap();
        buffer.truncate(n);
        
        // Should be an integer response
        assert!(buffer.starts_with(b":"));
    }
    
    let elapsed = start_time.elapsed();
    let ops_per_sec = (operation_count as f64 / elapsed.as_secs_f64()) as u64;
    
    println!("Atomic operations throughput: {} ops/sec", ops_per_sec);
    
    // Atomic operations should still be reasonably fast
    let min_atomic_throughput = MIN_THROUGHPUT_OPS_PER_SEC / 2; // Allow 50% of regular throughput
    assert!(ops_per_sec >= min_atomic_throughput,
           "Atomic operations throughput regression: {} ops/sec < {} ops/sec", 
           ops_per_sec, min_atomic_throughput);
    
    // Verify final counter value
    let get_cmd = b"*2\r\n$3\r\nGET\r\n$15\r\natomic_counter\r\n";
    stream.write_all(get_cmd).await.unwrap();
    stream.flush().await.unwrap();
    
    let mut buffer = vec![0u8; 1024];
    let n = stream.read(&mut buffer).await.unwrap();
    buffer.truncate(n);
    
    let expected = format!("${}\r\n{}\r\n", operation_count.to_string().len(), operation_count);
    assert_eq!(buffer, expected.as_bytes());
}

#[tokio::test]
async fn test_connection_overhead_performance() {
    let (_server, addr) = create_performance_test_server().await;
    
    let connection_count = 100;
    let operations_per_connection = 10;
    let start_time = Instant::now();
    
    let mut handles = Vec::new();
    
    // Test rapid connection creation and usage
    for i in 0..connection_count {
        let handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            
            // Perform a few operations per connection
            for j in 0..operations_per_connection {
                let key = format!("conn_{}_{}", i, j);
                let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n$5\r\nvalue\r\n", key.len(), key);
                
                stream.write_all(set_cmd.as_bytes()).await.unwrap();
                stream.flush().await.unwrap();
                
                let mut buffer = vec![0u8; 1024];
                let n = stream.read(&mut buffer).await.unwrap();
                buffer.truncate(n);
                assert_eq!(buffer, b"+OK\r\n");
            }
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all connections to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    let elapsed = start_time.elapsed();
    let total_operations = connection_count * operations_per_connection;
    let ops_per_sec = (total_operations as f64 / elapsed.as_secs_f64()) as u64;
    
    println!("Connection overhead test: {} ops/sec with {} connections", ops_per_sec, connection_count);
    
    // Should still maintain reasonable throughput despite connection overhead
    let min_connection_throughput = MIN_THROUGHPUT_OPS_PER_SEC / 4; // Allow 25% of regular throughput
    assert!(ops_per_sec >= min_connection_throughput,
           "Connection overhead performance regression: {} ops/sec < {} ops/sec", 
           ops_per_sec, min_connection_throughput);
}

#[tokio::test]
async fn test_ttl_operations_performance() {
    let (_server, addr) = create_performance_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();
    
    let key_count = 1000;
    
    // First, set many keys
    for i in 0..key_count {
        let key = format!("ttl_perf_{}", i);
        let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n$5\r\nvalue\r\n", key.len(), key);
        
        stream.write_all(set_cmd.as_bytes()).await.unwrap();
        stream.flush().await.unwrap();
        
        let mut buffer = vec![0u8; 1024];
        let n = stream.read(&mut buffer).await.unwrap();
        buffer.truncate(n);
        assert_eq!(buffer, b"+OK\r\n");
    }
    
    // Measure EXPIRE operation performance
    let start_time = Instant::now();
    
    for i in 0..key_count {
        let key = format!("ttl_perf_{}", i);
        let expire_cmd = format!("*3\r\n$6\r\nEXPIRE\r\n${}\r\n{}\r\n$2\r\n60\r\n", key.len(), key);
        
        stream.write_all(expire_cmd.as_bytes()).await.unwrap();
        stream.flush().await.unwrap();
        
        let mut buffer = vec![0u8; 1024];
        let n = stream.read(&mut buffer).await.unwrap();
        buffer.truncate(n);
        assert_eq!(buffer, b":1\r\n");
    }
    
    let expire_elapsed = start_time.elapsed();
    let expire_ops_per_sec = (key_count as f64 / expire_elapsed.as_secs_f64()) as u64;
    
    // Measure TTL operation performance
    let start_time = Instant::now();
    
    for i in 0..key_count {
        let key = format!("ttl_perf_{}", i);
        let ttl_cmd = format!("*2\r\n$3\r\nTTL\r\n${}\r\n{}\r\n", key.len(), key);
        
        stream.write_all(ttl_cmd.as_bytes()).await.unwrap();
        stream.flush().await.unwrap();
        
        let mut buffer = vec![0u8; 1024];
        let n = stream.read(&mut buffer).await.unwrap();
        buffer.truncate(n);
        
        // Should return a positive TTL value
        assert!(buffer.starts_with(b":"));
        let response_str = String::from_utf8_lossy(&buffer);
        let ttl_value: i64 = response_str.trim_start_matches(':').trim_end_matches("\r\n").parse().unwrap();
        assert!(ttl_value > 0 && ttl_value <= 60);
    }
    
    let ttl_elapsed = start_time.elapsed();
    let ttl_ops_per_sec = (key_count as f64 / ttl_elapsed.as_secs_f64()) as u64;
    
    println!("EXPIRE throughput: {} ops/sec", expire_ops_per_sec);
    println!("TTL throughput: {} ops/sec", ttl_ops_per_sec);
    
    // TTL operations should be reasonably fast
    let min_ttl_throughput = MIN_THROUGHPUT_OPS_PER_SEC / 2; // Allow 50% of regular throughput
    assert!(expire_ops_per_sec >= min_ttl_throughput,
           "EXPIRE performance regression: {} ops/sec < {} ops/sec", 
           expire_ops_per_sec, min_ttl_throughput);
    
    assert!(ttl_ops_per_sec >= min_ttl_throughput,
           "TTL performance regression: {} ops/sec < {} ops/sec", 
           ttl_ops_per_sec, min_ttl_throughput);
}

#[tokio::test]
async fn test_mixed_workload_performance() {
    let (_server, addr) = create_performance_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();
    
    let operation_count = 5000;
    let start_time = Instant::now();
    
    // Mixed workload: 50% SET, 30% GET, 15% INCR, 5% TTL operations
    for i in 0..operation_count {
        match i % 20 {
            0..=9 => {
                // SET operations (50%)
                let key = format!("mixed_{}", i);
                let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n$5\r\nvalue\r\n", key.len(), key);
                
                stream.write_all(set_cmd.as_bytes()).await.unwrap();
                stream.flush().await.unwrap();
                
                let mut buffer = vec![0u8; 1024];
                let n = stream.read(&mut buffer).await.unwrap();
                buffer.truncate(n);
                assert_eq!(buffer, b"+OK\r\n");
            }
            10..=15 => {
                // GET operations (30%)
                let key = format!("mixed_{}", i - 10);
                let get_cmd = format!("*2\r\n$3\r\nGET\r\n${}\r\n{}\r\n", key.len(), key);
                
                stream.write_all(get_cmd.as_bytes()).await.unwrap();
                stream.flush().await.unwrap();
                
                let mut buffer = vec![0u8; 1024];
                let n = stream.read(&mut buffer).await.unwrap();
                buffer.truncate(n);
                // Response could be value or nil
            }
            16..=18 => {
                // INCR operations (15%)
                let counter = format!("mixed_counter_{}", i % 5);
                let incr_cmd = format!("*2\r\n$4\r\nINCR\r\n${}\r\n{}\r\n", counter.len(), counter);
                
                stream.write_all(incr_cmd.as_bytes()).await.unwrap();
                stream.flush().await.unwrap();
                
                let mut buffer = vec![0u8; 1024];
                let n = stream.read(&mut buffer).await.unwrap();
                buffer.truncate(n);
                assert!(buffer.starts_with(b":"));
            }
            19 => {
                // TTL operations (5%)
                let key = format!("mixed_{}", i - 19);
                let ttl_cmd = format!("*2\r\n$3\r\nTTL\r\n${}\r\n{}\r\n", key.len(), key);
                
                stream.write_all(ttl_cmd.as_bytes()).await.unwrap();
                stream.flush().await.unwrap();
                
                let mut buffer = vec![0u8; 1024];
                let n = stream.read(&mut buffer).await.unwrap();
                buffer.truncate(n);
                assert!(buffer.starts_with(b":"));
            }
            _ => unreachable!(),
        }
    }
    
    let elapsed = start_time.elapsed();
    let ops_per_sec = (operation_count as f64 / elapsed.as_secs_f64()) as u64;
    
    println!("Mixed workload throughput: {} ops/sec", ops_per_sec);
    
    // Mixed workload should maintain reasonable performance
    let min_mixed_throughput = MIN_THROUGHPUT_OPS_PER_SEC / 2; // Allow 50% of regular throughput
    assert!(ops_per_sec >= min_mixed_throughput,
           "Mixed workload performance regression: {} ops/sec < {} ops/sec", 
           ops_per_sec, min_mixed_throughput);
}