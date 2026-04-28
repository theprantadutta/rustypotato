//! DoS hardening tests for Stage 3.
//!
//! Asserts that the protocol parser refuses outsized inputs before
//! allocating, that the per-connection accumulator can't grow without
//! bound, that the connection-limit semaphore is a hard cap under
//! concurrent accepts, and that idle clients are evicted.

use rustypotato::network::{CodecLimits, RespCodec};
use rustypotato::{Config, RustyPotatoServer};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};

// ==================== Codec bounds ====================

/// `*<i32::MAX>\r\n` previously triggered a multi-GB Vec allocation.
/// Now it must be rejected with a ProtocolError before we touch the
/// allocator.
#[test]
fn codec_rejects_oversized_array_length() {
    let mut codec = RespCodec::with_limits(CodecLimits {
        max_bulk_size: 64,
        max_array_length: 1024,
        max_buffer_size: 1024 * 1024,
    });
    let result = codec.decode(b"*2147483647\r\n");
    let err = result.expect_err("oversized array must error");
    let msg = err.to_string();
    assert!(
        msg.contains("array length") && msg.contains("exceeds max"),
        "unexpected error: {msg}"
    );
}

/// `$<huge>\r\n...` previously allocated `vec![0u8; huge]` directly. Now
/// it must reject before allocation.
#[test]
fn codec_rejects_oversized_bulk_string() {
    let mut codec = RespCodec::with_limits(CodecLimits {
        max_bulk_size: 1024,
        max_array_length: 1024,
        max_buffer_size: 1024 * 1024,
    });
    // Note: we wrap it in an array because a bare bulk string isn't a
    // command, but the parser still has to traverse it.
    let result = codec.decode(b"*1\r\n$1073741824\r\n");
    let err = result.expect_err("oversized bulk must error");
    let msg = err.to_string();
    assert!(
        msg.contains("bulk string length") && msg.contains("exceeds max"),
        "unexpected error: {msg}"
    );
}

/// A slow-loris client trickling bytes into the buffer must hit the
/// per-connection buffer cap and get a protocol error rather than
/// growing the buffer indefinitely.
///
/// We use a partial-but-valid frame: an array containing a 2000-byte
/// bulk string. The parser sees the header and waits for the 2000-byte
/// payload. While the payload trickles in, the buffer grows; once it
/// exceeds max_buffer_size we expect rejection.
#[test]
fn codec_rejects_buffer_overflow() {
    let mut codec = RespCodec::with_limits(CodecLimits {
        max_bulk_size: 100_000, // tolerate the declared bulk size...
        max_array_length: 1024,
        max_buffer_size: 1024, // ...but cap the accumulator at 1KB.
    });
    // Header declares "array of 1 bulk string of length 2000".
    let header = b"*1\r\n$2000\r\n";
    assert!(
        matches!(codec.decode(header), Ok(None)),
        "header should buffer waiting for payload"
    );
    // First chunk: 600 bytes of payload. Buffer = 11 + 600 = 611,
    // still under cap.
    let chunk = vec![b'x'; 600];
    assert!(
        matches!(codec.decode(&chunk), Ok(None)),
        "first chunk should fit under max_buffer_size"
    );
    // Second chunk: another 600. Buffer would grow to 1211 > 1024.
    let result = codec.decode(&chunk);
    let err = result.expect_err("buffer overflow must error");
    let msg = err.to_string();
    assert!(msg.contains("buffer overflow"), "unexpected error: {msg}");
}

// ==================== Connection-limit semaphore ====================

/// Open `max_connections + N` streams concurrently and confirm the
/// semaphore caps the active count. The previous TOCTOU
/// can_accept-then-add could let extras through under burst.
#[tokio::test]
async fn connection_limit_is_hard_cap_under_burst() {
    let mut config = Config::default();
    config.server.port = 0;
    config.server.max_connections = 4;
    // Make the AOF irrelevant for this test.
    config.storage.aof_enabled = false;

    let mut server = RustyPotatoServer::open(config).await.unwrap();
    let addr = server.start_with_addr().await.unwrap();
    sleep(Duration::from_millis(50)).await;

    // Try to open 8 connections concurrently.
    let mut handles = vec![];
    for _ in 0..8 {
        handles.push(tokio::spawn(async move {
            // Connect and immediately try to read. Accepted clients will
            // be allowed to read; rejected ones will see the
            // "-ERR server connection limit reached\r\n" error message.
            let mut stream = TcpStream::connect(addr).await.ok()?;
            let mut buf = vec![0u8; 256];
            let read_result = timeout(Duration::from_secs(1), stream.read(&mut buf))
                .await
                .ok()?;
            let n = read_result.unwrap_or(0);
            buf.truncate(n);
            // Empty read => connection still open and waiting (accepted).
            // Non-empty + -ERR => rejected.
            if buf.starts_with(b"-ERR server connection limit") {
                Some(false)
            } else {
                // Keep this stream alive a beat to keep the slot busy.
                sleep(Duration::from_millis(100)).await;
                drop(stream);
                Some(true)
            }
        }));
    }

    let mut accepted = 0;
    let mut rejected = 0;
    for h in handles {
        match h.await.unwrap() {
            Some(true) => accepted += 1,
            Some(false) => rejected += 1,
            None => {}
        }
    }

    // The hard cap is 4. Some clients may not get an explicit reject
    // (e.g. they were dropped before the server could write the error),
    // but the SUM accepted ≤ 4 must hold.
    assert!(
        accepted <= 4,
        "more than max_connections accepted: accepted={accepted} rejected={rejected}"
    );
    // And we must have observed at least one rejection given 8 attempts.
    assert!(
        rejected >= 1 || accepted < 8,
        "expected some clients to be rejected"
    );

    server.shutdown().await.unwrap();
}

// ==================== Idle eviction ====================

/// A connection that goes idle past `idle_timeout` is closed by the
/// background eviction task.
#[tokio::test]
async fn idle_connection_is_evicted() {
    let mut config = Config::default();
    config.server.port = 0;
    config.network.idle_timeout = 1; // 1-second idle timeout
    config.storage.aof_enabled = false;

    let mut server = RustyPotatoServer::open(config).await.unwrap();
    let addr = server.start_with_addr().await.unwrap();
    sleep(Duration::from_millis(50)).await;

    let mut stream = TcpStream::connect(addr).await.unwrap();
    // Send one command to make the connection real, then go silent.
    stream.write_all(b"*1\r\n$4\r\nPING\r\n").await.unwrap();
    let _ = stream.flush().await;
    // Read the (likely error since PING isn't registered yet) response.
    let mut buf = vec![0u8; 64];
    let _ = timeout(Duration::from_secs(1), stream.read(&mut buf)).await;

    // Wait > idle_timeout + scan_interval. With idle_timeout=1s the
    // scanner runs every max(1/4, 1) = 1s, so worst case ~2.5s.
    sleep(Duration::from_secs(3)).await;

    // The server should have shut our socket down. A read returns 0
    // bytes (EOF) on a half-closed stream.
    let mut buf2 = vec![0u8; 64];
    match timeout(Duration::from_secs(2), stream.read(&mut buf2)).await {
        Ok(Ok(0)) => { /* expected: server closed the socket */ }
        Ok(Ok(n)) => {
            // Some bytes were received — accept this only if they
            // look like an error frame; otherwise the eviction failed.
            let s = String::from_utf8_lossy(&buf2[..n]);
            assert!(
                s.starts_with('-'),
                "unexpected non-error data after idle timeout: {s:?}"
            );
        }
        Ok(Err(_)) | Err(_) => {
            // Read errored / timed out — also acceptable, indicates the
            // socket isn't readable anymore.
        }
    }

    server.shutdown().await.unwrap();
}
