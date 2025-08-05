//! Simple server test to debug the internal server error

use rustypotato::{Config, RustyPotatoServer};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::test]
async fn test_simple_server_operations() {
    // Create server with minimal configuration
    let mut config = Config::default();
    config.server.port = 0;
    config.storage.aof_enabled = false;

    let mut server = RustyPotatoServer::new(config).unwrap();
    let addr = server.start_with_addr().await.unwrap();

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Test simple SET command
    let set_cmd = b"*3\r\n$3\r\nSET\r\n$4\r\ntest\r\n$5\r\nvalue\r\n";
    stream.write_all(set_cmd).await.unwrap();
    stream.flush().await.unwrap();

    let mut buffer = vec![0u8; 1024];
    let n = stream.read(&mut buffer).await.unwrap();
    buffer.truncate(n);

    println!("SET response: {:?}", String::from_utf8_lossy(&buffer));

    // Test simple GET command
    let get_cmd = b"*2\r\n$3\r\nGET\r\n$4\r\ntest\r\n";
    stream.write_all(get_cmd).await.unwrap();
    stream.flush().await.unwrap();

    let mut buffer = vec![0u8; 1024];
    let n = stream.read(&mut buffer).await.unwrap();
    buffer.truncate(n);

    println!("GET response: {:?}", String::from_utf8_lossy(&buffer));
}
