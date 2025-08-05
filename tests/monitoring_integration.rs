//! Integration tests for monitoring and logging infrastructure
//!
//! This module tests the complete monitoring system including health checks,
//! log rotation, and metrics endpoints.

use rustypotato::monitoring::{LogRotationConfig, RotationPolicy};
use rustypotato::{
    HealthChecker, LogRotationManager, MemoryStore, MetricsCollector, MonitoringServer,
};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::fs;
// Removed unused imports

#[tokio::test]
async fn test_health_checker_integration() {
    let storage = Arc::new(MemoryStore::new());
    let metrics = Arc::new(MetricsCollector::new());
    let health_checker = Arc::new(HealthChecker::new(
        Arc::clone(&storage),
        Arc::clone(&metrics),
    ));

    // Test basic health check
    let health_result = health_checker.check_all_components().await;
    assert!(health_result.is_healthy());
    assert!(!health_result.status().components.is_empty());

    // Test individual component checks
    assert!(health_checker.is_ready().await);
    assert!(health_checker.is_alive().await);

    // Test cached status
    let cached = health_checker.get_cached_status().await;
    assert!(cached.is_some());
}

#[tokio::test]
async fn test_log_rotation_integration() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("test_rotation.log");

    // Create initial log content
    let initial_content = "Initial log content\n".repeat(100);
    fs::write(&log_path, &initial_content).await.unwrap();

    let config = LogRotationConfig {
        log_file_path: log_path.clone(),
        rotation_policy: RotationPolicy::Size(500), // Small size for testing
        max_files: 3,
        compress: false,
        rotation_dir: Some(temp_dir.path().to_path_buf()),
    };

    let rotation_manager = Arc::new(LogRotationManager::new(config));

    // Test manual rotation
    rotation_manager.rotate_now().await.unwrap();

    // Check that rotation occurred
    let status = rotation_manager.get_status().await;
    assert!(status.last_rotation.is_some());
    assert_eq!(status.current_file_size, 0); // New file should be empty

    // Check that rotated file exists
    let mut entries = fs::read_dir(temp_dir.path()).await.unwrap();
    let mut found_rotated = false;

    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Ok(name) = entry.file_name().into_string() {
            if name.starts_with("test_rotation.log.") && name != "test_rotation.log" {
                found_rotated = true;
                break;
            }
        }
    }

    assert!(found_rotated, "Rotated file not found");
}

#[tokio::test]
async fn test_monitoring_server_integration() {
    let storage = Arc::new(MemoryStore::new());
    let metrics = Arc::new(MetricsCollector::new());
    let health_checker = Arc::new(HealthChecker::new(
        Arc::clone(&storage),
        Arc::clone(&metrics),
    ));

    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("monitoring_test.log");

    let rotation_config = LogRotationConfig {
        log_file_path: log_path,
        rotation_policy: RotationPolicy::Manual,
        max_files: 5,
        compress: false,
        rotation_dir: Some(temp_dir.path().to_path_buf()),
    };

    let log_rotation = Arc::new(LogRotationManager::new(rotation_config));

    let monitoring_server = MonitoringServer::new(
        health_checker,
        metrics,
        log_rotation,
        "127.0.0.1".to_string(),
        0, // Use any available port
    );

    // Start server in background
    let server_handle = tokio::spawn(async move { monitoring_server.start().await });

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Test would require actual HTTP client implementation
    // For now, just verify the server can be created and started

    // Cancel the server task
    server_handle.abort();
}

#[tokio::test]
async fn test_health_check_with_storage_operations() {
    let storage = Arc::new(MemoryStore::new());
    let metrics = Arc::new(MetricsCollector::new());

    // Add some data to storage
    storage.set("test_key", "test_value").await.unwrap();

    // Record some metrics
    metrics
        .record_command_latency("SET", Duration::from_micros(100))
        .await;
    metrics
        .record_command_latency("GET", Duration::from_micros(50))
        .await;

    let health_checker = HealthChecker::new(storage, metrics);

    let health_result = health_checker.check_all_components().await;

    // Should be healthy with actual data
    assert!(health_result.is_healthy());

    // Check that storage component shows healthy
    let storage_health = health_result.status().components.get("storage").unwrap();
    assert_eq!(storage_health.status, "healthy");
    assert!(storage_health.details.contains_key("test_operation"));

    // Check that metrics component shows healthy
    let metrics_health = health_result.status().components.get("metrics").unwrap();
    assert_eq!(metrics_health.status, "healthy");
    assert!(metrics_health.details.contains_key("total_operations"));
}

#[tokio::test]
async fn test_log_rotation_with_compression() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("compress_test.log");

    // Create log content
    let content = "This is test log content that should be compressed\n".repeat(50);
    fs::write(&log_path, &content).await.unwrap();

    let config = LogRotationConfig {
        log_file_path: log_path.clone(),
        rotation_policy: RotationPolicy::Manual,
        max_files: 3,
        compress: true,
        rotation_dir: Some(temp_dir.path().to_path_buf()),
    };

    let rotation_manager = LogRotationManager::new(config);

    // Perform rotation with compression
    rotation_manager.rotate_now().await.unwrap();

    // Check for compressed file
    let mut entries = fs::read_dir(temp_dir.path()).await.unwrap();
    let mut found_compressed = false;

    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Ok(name) = entry.file_name().into_string() {
            if name.starts_with("compress_test.log.") && name.ends_with(".gz") {
                found_compressed = true;

                // Verify the compressed file is smaller than original
                let compressed_size = entry.metadata().await.unwrap().len();
                assert!(compressed_size < content.len() as u64);
                break;
            }
        }
    }

    assert!(found_compressed, "Compressed file not found");
}

#[tokio::test]
async fn test_log_rotation_cleanup() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("cleanup_test.log");

    let config = LogRotationConfig {
        log_file_path: log_path.clone(),
        rotation_policy: RotationPolicy::Manual,
        max_files: 2, // Keep only 2 files
        compress: false,
        rotation_dir: Some(temp_dir.path().to_path_buf()),
    };

    let rotation_manager = LogRotationManager::new(config);

    // Create multiple rotations
    for i in 0..5 {
        let content = format!("Log content iteration {i}\n").repeat(10);
        fs::write(&log_path, &content).await.unwrap();

        // Add small delay to ensure different timestamps
        tokio::time::sleep(Duration::from_millis(10)).await;

        rotation_manager.rotate_now().await.unwrap();
    }

    // Count rotated files
    let mut entries = fs::read_dir(temp_dir.path()).await.unwrap();
    let mut rotated_count = 0;

    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Ok(name) = entry.file_name().into_string() {
            if name.starts_with("cleanup_test.log.") && name != "cleanup_test.log" {
                rotated_count += 1;
            }
        }
    }

    // Should have at most max_files (2) rotated files
    assert!(
        rotated_count <= 2,
        "Too many rotated files: {rotated_count}"
    );
}

#[tokio::test]
async fn test_metrics_collection_accuracy() {
    let metrics = Arc::new(MetricsCollector::new());

    // Record various operations
    metrics
        .record_command_latency("SET", Duration::from_micros(100))
        .await;
    metrics
        .record_command_latency("GET", Duration::from_micros(50))
        .await;
    metrics
        .record_command_latency("SET", Duration::from_micros(120))
        .await;

    metrics.record_network_bytes(1024, 512).await;
    metrics.record_memory_usage(1024 * 1024).await;

    let summary = metrics.get_summary().await;

    // Verify accuracy
    assert_eq!(summary.total_operations, 3);
    assert_eq!(summary.total_commands, 3);
    assert_eq!(summary.total_bytes_read, 1024);
    assert_eq!(summary.total_bytes_written, 512);
    assert_eq!(summary.memory_usage_bytes, 1024 * 1024);

    // Check that most frequent command is correct
    let (most_frequent_cmd, count) = summary.most_frequent_command.unwrap();
    assert_eq!(most_frequent_cmd, "SET");
    assert_eq!(count, 2);

    // Check latency calculations
    assert!(summary.average_latency.is_some());
    assert!(summary.p95_latency.is_some());
    assert!(summary.operations_per_second > 0.0);
}

#[tokio::test]
async fn test_health_check_component_details() {
    let storage = Arc::new(MemoryStore::new());
    let metrics = Arc::new(MetricsCollector::new());

    // Add some test data
    storage.set("health_test", "value").await.unwrap();
    metrics
        .record_command_latency("TEST", Duration::from_micros(75))
        .await;

    let health_checker = HealthChecker::new(storage, metrics);
    let health_result = health_checker.check_all_components().await;

    // Check storage component details
    let storage_component = health_result.status().components.get("storage").unwrap();
    assert_eq!(storage_component.status, "healthy");
    assert!(storage_component.details.contains_key("test_operation"));
    // Response time should be a valid number (no need to check >= 0 for u64)

    // Check metrics component details
    let metrics_component = health_result.status().components.get("metrics").unwrap();
    assert_eq!(metrics_component.status, "healthy");
    assert!(metrics_component.details.contains_key("total_operations"));
    assert!(metrics_component.details.contains_key("uptime_seconds"));

    // Check memory component details
    let memory_component = health_result.status().components.get("memory").unwrap();
    assert!(memory_component
        .details
        .contains_key("current_memory_bytes"));
    assert!(memory_component.details.contains_key("peak_memory_bytes"));

    // Check network component details
    let network_component = health_result.status().components.get("network").unwrap();
    assert!(network_component.details.contains_key("active_connections"));
    assert!(network_component
        .details
        .contains_key("connection_rejection_rate_percent"));

    // Check disk component details
    let disk_component = health_result.status().components.get("disk").unwrap();
    assert!(disk_component.details.contains_key("disk_write_test"));
}

#[tokio::test]
async fn test_log_rotation_status_tracking() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("status_test.log");

    // Create initial log file
    fs::write(&log_path, "Initial content").await.unwrap();

    let config = LogRotationConfig {
        log_file_path: log_path.clone(),
        rotation_policy: RotationPolicy::Manual,
        max_files: 5,
        compress: false,
        rotation_dir: Some(temp_dir.path().to_path_buf()),
    };

    let rotation_manager = LogRotationManager::new(config);

    // Check initial status
    let initial_status = rotation_manager.get_status().await;
    assert!(initial_status.last_rotation.is_none());
    assert_eq!(
        initial_status.current_file_size,
        "Initial content".len() as u64
    );
    assert_eq!(initial_status.rotated_files_count, 0);

    // Perform rotation
    rotation_manager.rotate_now().await.unwrap();

    // Check updated status
    let updated_status = rotation_manager.get_status().await;
    assert!(updated_status.last_rotation.is_some());
    assert_eq!(updated_status.current_file_size, 0); // New file should be empty
    assert_eq!(updated_status.rotated_files_count, 1);
    assert!(updated_status.total_rotated_size > 0);
}

#[tokio::test]
async fn test_monitoring_endpoints_format() {
    // This test verifies the format of monitoring responses
    // without actually starting a server

    let storage = Arc::new(MemoryStore::new());
    let metrics = Arc::new(MetricsCollector::new());

    // Add test data
    storage.set("test", "value").await.unwrap();
    metrics
        .record_command_latency("SET", Duration::from_micros(100))
        .await;

    let health_checker = Arc::new(HealthChecker::new(storage, metrics.clone()));

    // Test health check response format
    let health_result = health_checker.check_all_components().await;
    let health_json = serde_json::to_string(&health_result.status()).unwrap();

    // Verify JSON structure
    let parsed: serde_json::Value = serde_json::from_str(&health_json).unwrap();
    assert!(parsed.get("status").is_some());
    assert!(parsed.get("timestamp").is_some());
    assert!(parsed.get("components").is_some());
    assert!(parsed.get("summary").is_some());

    // Test metrics summary format
    let summary = metrics.get_summary().await;
    let summary_json = serde_json::to_string(&summary).unwrap();

    let parsed_summary: serde_json::Value = serde_json::from_str(&summary_json).unwrap();
    assert!(parsed_summary.get("total_operations").is_some());
    assert!(parsed_summary.get("operations_per_second").is_some());
    assert!(parsed_summary.get("uptime").is_some());
}
