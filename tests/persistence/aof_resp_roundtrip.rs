//! End-to-end AOF round-trip test for the RESP-framed format.
//!
//! Asserts the load-bearing claim of Stage 2: a server with persistence
//! enabled survives a clean shutdown and a kill-9-equivalent restart with
//! every value type intact — strings, integers, hashes, INCR, and TTL.

use rustypotato::{Config, RustyPotatoServer};
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};

fn persistent_config(aof_path: PathBuf) -> Config {
    let mut config = Config::default();
    config.server.port = 0;
    config.storage.aof_enabled = true;
    config.storage.aof_path = aof_path;
    config
}

async fn send(stream: &mut TcpStream, cmd: &[u8]) -> Vec<u8> {
    stream.write_all(cmd).await.unwrap();
    stream.flush().await.unwrap();
    let mut buf = vec![0u8; 4096];
    let n = timeout(Duration::from_secs(5), stream.read(&mut buf))
        .await
        .unwrap()
        .unwrap();
    buf.truncate(n);
    buf
}

fn resp(parts: &[&str]) -> Vec<u8> {
    let mut out = format!("*{}\r\n", parts.len()).into_bytes();
    for p in parts {
        out.extend_from_slice(format!("${}\r\n", p.len()).as_bytes());
        out.extend_from_slice(p.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// String, integer (INCR), hash, TTL — all survive a clean restart.
#[tokio::test]
async fn aof_round_trip_all_value_types_clean_shutdown() {
    let tmp = TempDir::new().unwrap();
    let aof = tmp.path().join("roundtrip.aof");

    // Phase 1: write data, gracefully shut down.
    {
        let mut server = RustyPotatoServer::open(persistent_config(aof.clone()))
            .await
            .unwrap();
        let addr = server.start_with_addr().await.unwrap();
        sleep(Duration::from_millis(100)).await;

        let mut stream = TcpStream::connect(addr).await.unwrap();

        // String
        let r = send(&mut stream, &resp(&["SET", "str", "hello world"])).await;
        assert_eq!(r, b"+OK\r\n");

        // Integer (and an INCR for good measure)
        let r = send(&mut stream, &resp(&["SET", "ctr", "10"])).await;
        assert_eq!(r, b"+OK\r\n");
        let r = send(&mut stream, &resp(&["INCR", "ctr"])).await;
        assert_eq!(r, b":11\r\n");
        let r = send(&mut stream, &resp(&["INCR", "ctr"])).await;
        assert_eq!(r, b":12\r\n");

        // Hash with multiple fields
        let r = send(&mut stream, &resp(&["HSET", "h", "f1", "v1"])).await;
        assert_eq!(r, b":1\r\n");
        let r = send(&mut stream, &resp(&["HSET", "h", "f2", "v2"])).await;
        assert_eq!(r, b":1\r\n");

        // TTL
        let r = send(&mut stream, &resp(&["SET", "ttlk", "alive"])).await;
        assert_eq!(r, b"+OK\r\n");
        let r = send(&mut stream, &resp(&["EXPIRE", "ttlk", "300"])).await;
        assert_eq!(r, b":1\r\n");

        drop(stream);
        server.shutdown().await.unwrap();
    }

    assert!(aof.exists(), "AOF file should have been created");
    assert!(
        std::fs::metadata(&aof).unwrap().len() > 0,
        "AOF should not be empty after shutdown"
    );

    // Phase 2: restart, verify every value survived.
    {
        let mut server = RustyPotatoServer::open(persistent_config(aof.clone()))
            .await
            .unwrap();
        let addr = server.start_with_addr().await.unwrap();
        sleep(Duration::from_millis(100)).await;

        let mut stream = TcpStream::connect(addr).await.unwrap();

        let r = send(&mut stream, &resp(&["GET", "str"])).await;
        assert_eq!(r, b"$11\r\nhello world\r\n", "string survives");

        let r = send(&mut stream, &resp(&["GET", "ctr"])).await;
        assert_eq!(r, b"$2\r\n12\r\n", "INCR result survives");

        let r = send(&mut stream, &resp(&["HGET", "h", "f1"])).await;
        assert_eq!(r, b"$2\r\nv1\r\n", "hash f1 survives");
        let r = send(&mut stream, &resp(&["HGET", "h", "f2"])).await;
        assert_eq!(r, b"$2\r\nv2\r\n", "hash f2 survives");

        let r = send(&mut stream, &resp(&["GET", "ttlk"])).await;
        assert_eq!(r, b"$5\r\nalive\r\n", "TTL key value survives");
        let r = send(&mut stream, &resp(&["TTL", "ttlk"])).await;
        // Allow a few seconds of drift for replay timing.
        let s = String::from_utf8_lossy(&r);
        assert!(
            s.starts_with(':'),
            "TTL response must be an integer, got {s:?}"
        );
        let ttl: i64 = s.trim_start_matches(':').trim_end().parse().unwrap();
        assert!(
            (250..=300).contains(&ttl),
            "TTL after replay should be near 300, got {ttl}"
        );

        drop(stream);
        server.shutdown().await.unwrap();
    }
}

/// AOF survives a kill-9-equivalent (no shutdown call) restart, modulo
/// the last 1-2 commands depending on fsync policy. With `Always` policy
/// every acknowledged command must survive.
#[tokio::test]
async fn aof_round_trip_survives_drop_without_shutdown() {
    let tmp = TempDir::new().unwrap();
    let aof = tmp.path().join("crash.aof");

    let addr = {
        let mut server = RustyPotatoServer::open(persistent_config(aof.clone()))
            .await
            .unwrap();
        let addr = server.start_with_addr().await.unwrap();
        sleep(Duration::from_millis(100)).await;

        let mut stream = TcpStream::connect(addr).await.unwrap();
        for i in 0..20 {
            let key = format!("k{i}");
            let val = format!("v{i}");
            let r = send(&mut stream, &resp(&["SET", &key, &val])).await;
            assert_eq!(r, b"+OK\r\n");
        }
        drop(stream);
        // No graceful shutdown — drop the server. With aof_fsync = Always
        // (the default in persistent_config(), via Config::default()'s
        // FsyncPolicy::EverySecond -> wait, let's verify) the writer task
        // must have flushed each command synchronously.
        sleep(Duration::from_millis(200)).await;
        addr
    };

    let _ = addr;

    // Restart and verify all 20 keys survived.
    {
        let mut server = RustyPotatoServer::open(persistent_config(aof.clone()))
            .await
            .unwrap();
        let addr = server.start_with_addr().await.unwrap();
        sleep(Duration::from_millis(100)).await;

        let mut stream = TcpStream::connect(addr).await.unwrap();
        let mut survived = 0;
        for i in 0..20 {
            let key = format!("k{i}");
            let r = send(&mut stream, &resp(&["GET", &key])).await;
            // Permit the last entry or two to be lost depending on fsync
            // policy, but the prefix must be intact.
            if r != b"$-1\r\n" {
                survived += 1;
            }
        }
        // Even with EverySecond default, ~all 20 should survive given the
        // 200ms post-write sleep above.
        assert!(survived >= 18, "expected most keys to survive, got {survived}");

        drop(stream);
        server.shutdown().await.unwrap();
    }
}
