# RustyPotato 🥔

[![CI](https://github.com/theprantadutta/rustypotato/actions/workflows/ci.yml/badge.svg)](https://github.com/theprantadutta/rustypotato/actions/workflows/ci.yml)
[![Release](https://github.com/theprantadutta/rustypotato/actions/workflows/release.yml/badge.svg)](https://github.com/theprantadutta/rustypotato/actions/workflows/release.yml)
[![Docker](https://img.shields.io/docker/pulls/theprantadutta/rustypotato)](https://hub.docker.com/r/theprantadutta/rustypotato)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)

A high-performance, Redis-compatible key-value store built in Rust with unique features for modern applications.

## 🚀 What is RustyPotato?

RustyPotato is a next-generation key-value store that combines Redis compatibility with innovative features powered by Rust's safety and performance guarantees. It's designed for developers who need the reliability of Redis with advanced capabilities like data branching, time travel debugging, and WebAssembly plugin support.

### Why RustyPotato?

- **🔥 Blazing Fast**: Built with Rust and async I/O for maximum performance
- **🛡️ Memory Safe**: Zero-cost abstractions with compile-time safety guarantees
- **🔌 Redis Compatible**: Drop-in replacement for Redis with RESP protocol support
- **🌟 Unique Features**: Git-style branching, time travel, WASM plugins, and more
- **📊 Production Ready**: Comprehensive monitoring, persistence, and clustering support

## 📋 Table of Contents

- [Features](#-features)
- [Quick Start](#-quick-start)
- [Installation](#-installation)
- [Usage](#-usage)
- [Configuration](#-configuration)
- [Docker](#-docker)
- [Development](#-development)
- [Performance](#-performance)
- [Contributing](#-contributing)
- [License](#-license)

## ✨ Features

### Core Features (✅ Implemented)

RustyPotato provides all essential Redis functionality with enhanced performance:

- **Basic Operations**: SET, GET, DEL, EXISTS with atomic guarantees
- **TTL Management**: EXPIRE, TTL with precise expiration handling
- **Atomic Operations**: INCR, DECR with overflow protection
- **Concurrent Storage**: Lock-free data structures for maximum throughput
- **Persistence**: Append-only file (AOF) with configurable fsync policies
- **Network Protocol**: Full Redis RESP protocol compatibility
- **CLI Client**: Interactive and batch command execution
- **Configuration**: File-based and environment variable configuration
- **Monitoring**: Built-in metrics, health checks, and structured logging
- **Error Handling**: Comprehensive error recovery and client feedback

### Bonus Features (🚧 Planned)

Advanced data structures and enterprise features:

- **Hash Operations**: HSET, HGET, HDEL, HGETALL for nested data
- **List Operations**: LPUSH, RPUSH, LPOP, RPOP for queues and stacks
- **Pub/Sub Messaging**: Real-time event-driven communication
- **LRU Cache Mode**: Memory-bounded operation with intelligent eviction
- **Keyspace Notifications**: React to key lifecycle events
- **Authentication**: Password-based security with role management
- **Compression**: Multiple algorithms (gzip, lz4, zstd) for storage efficiency
- **JSON Support**: Native JSON storage with dot-notation queries
- **Metrics API**: Prometheus-compatible metrics endpoint
- **Snapshots**: Background saves and point-in-time recovery

### Unique Features (🔮 Innovation)

Revolutionary capabilities that set RustyPotato apart:

- **🪝 Boiled Key Mode**: Attach custom transformation hooks to keys
- **🌳 Git-style Branching**: Create isolated data branches for testing and feature flags
- **⏰ Time Travel Debugging**: View operation history and rollback to any point in time
- **🔧 WebAssembly Plugins**: Extend functionality with custom WASM modules
- **🔍 Advanced Queries**: Pattern matching and complex query expressions
- **🌐 Distributed Consensus**: Raft-based clustering for high availability
- **🧠 ML-based Caching**: Intelligent eviction using machine learning
- **🌊 Stream Processing**: Real-time data transformations and routing
- **📈 Time-series Support**: Native temporal data types and operations
- **📋 Dynamic Schemas**: Runtime schema validation and evolution

## 🚀 Quick Start

### Using Docker (Recommended)

```bash
# Run RustyPotato server
docker run -d -p 6379:6379 --name rustypotato theprantadutta/rustypotato:latest

# Connect with CLI
docker run -it --rm --link rustypotato theprantadutta/rustypotato:latest rustypotato-cli --address rustypotato:6379
```

### Using Pre-built Binaries

```bash
# Download for your platform
curl -L https://github.com/theprantadutta/rustypotato/releases/latest/download/rustypotato-linux-x86_64.tar.gz | tar xz

# Run server
./rustypotato-server

# In another terminal, use CLI
./rustypotato-cli
```

### Building from Source

```bash
# Clone repository
git clone https://github.com/theprantadutta/rustypotato.git
cd rustypotato

# Build and run
cargo build --release
./target/release/rustypotato-server
```

## 📦 Installation

### Pre-built Binaries

Download the latest release for your platform:

| Platform | Architecture | Download |
|----------|-------------|----------|
| Linux | x86_64 | [rustypotato-linux-x86_64.tar.gz](https://github.com/theprantadutta/rustypotato/releases/latest/download/rustypotato-linux-x86_64.tar.gz) |
| Linux | aarch64 | [rustypotato-linux-aarch64.tar.gz](https://github.com/theprantadutta/rustypotato/releases/latest/download/rustypotato-linux-aarch64.tar.gz) |
| macOS | x86_64 | [rustypotato-macos-x86_64.tar.gz](https://github.com/theprantadutta/rustypotato/releases/latest/download/rustypotato-macos-x86_64.tar.gz) |
| macOS | aarch64 | [rustypotato-macos-aarch64.tar.gz](https://github.com/theprantadutta/rustypotato/releases/latest/download/rustypotato-macos-aarch64.tar.gz) |
| Windows | x86_64 | [rustypotato-windows-x86_64.zip](https://github.com/theprantadutta/rustypotato/releases/latest/download/rustypotato-windows-x86_64.zip) |

### Package Managers

```bash
# Homebrew (macOS/Linux)
brew install rustypotato/tap/rustypotato

# Cargo (Rust)
cargo install rustypotato

# Arch Linux (AUR)
yay -S rustypotato
```

### Building from Source

#### Prerequisites

- Rust 1.70+ (install via [rustup](https://rustup.rs/))
- Git

#### Build Steps

```bash
# Clone the repository
git clone https://github.com/theprantadutta/rustypotato.git
cd rustypotato

# Build optimized release
cargo build --release

# Install binaries
cargo install --path .

# Verify installation
rustypotato-server --version
rustypotato-cli --version
```

## 🎯 Usage

### Server

Start the RustyPotato server:

```bash
# Default configuration (port 6379)
rustypotato-server

# Custom port and configuration
rustypotato-server --port 6380 --config /path/to/config.toml

# With persistence enabled
rustypotato-server --aof-path /data/rustypotato.aof

# Enable metrics endpoint
rustypotato-server --metrics-port 9090
```

#### Server Options

```
USAGE:
    rustypotato-server [OPTIONS]

OPTIONS:
    -p, --port <PORT>              Server port [default: 6379]
    -b, --bind <ADDRESS>           Bind address [default: 127.0.0.1]
    -c, --config <FILE>            Configuration file path
        --aof-path <PATH>          AOF persistence file path
        --max-connections <NUM>    Maximum concurrent connections [default: 10000]
        --metrics-port <PORT>      Metrics HTTP server port
        --log-level <LEVEL>        Log level [default: info]
    -h, --help                     Print help information
    -V, --version                  Print version information
```

### CLI Client

#### Interactive Mode

```bash
# Connect to local server
rustypotato-cli

# Connect to remote server
rustypotato-cli --address 192.168.1.100:6379

# Interactive session
rustypotato> SET mykey "Hello, World!"
OK
rustypotato> GET mykey
"Hello, World!"
rustypotato> DEL mykey
(integer) 1
rustypotato> EXIT
```

#### Batch Mode

```bash
# Single command
rustypotato-cli set mykey "value"

# Multiple commands
rustypotato-cli set key1 "value1" set key2 "value2" get key1

# From file
rustypotato-cli --file commands.txt

# Pipeline mode
echo -e "SET key1 value1\nGET key1\nDEL key1" | rustypotato-cli --pipe
```

#### CLI Options

```
USAGE:
    rustypotato-cli [OPTIONS] [COMMAND]...

OPTIONS:
    -a, --address <ADDRESS>    Server address [default: 127.0.0.1:6379]
    -f, --file <FILE>          Execute commands from file
    -p, --pipe                 Read commands from stdin
        --timeout <SECONDS>    Connection timeout [default: 30]
        --format <FORMAT>      Output format [json|table|raw] [default: raw]
    -h, --help                 Print help information
    -V, --version              Print version information

COMMANDS:
    set <key> <value>          Set key to value
    get <key>                  Get value of key
    del <key>...               Delete one or more keys
    exists <key>               Check if key exists
    expire <key> <seconds>     Set key expiration
    ttl <key>                  Get key time-to-live
    incr <key>                 Increment key value
    decr <key>                 Decrement key value
    info                       Server information
    ping                       Test connection
    help                       Show this help message
```

### Programming Language Clients

RustyPotato is compatible with existing Redis clients:

#### Python (redis-py)

```python
import redis

# Connect to RustyPotato
r = redis.Redis(host='localhost', port=6379, decode_responses=True)

# Basic operations
r.set('mykey', 'Hello from Python!')
value = r.get('mykey')
print(value)  # Hello from Python!

# Atomic operations
r.incr('counter')
r.expire('counter', 60)  # Expire in 60 seconds
```

#### Node.js (ioredis)

```javascript
const Redis = require('ioredis');

// Connect to RustyPotato
const redis = new Redis({
  host: 'localhost',
  port: 6379,
});

// Basic operations
await redis.set('mykey', 'Hello from Node.js!');
const value = await redis.get('mykey');
console.log(value); // Hello from Node.js!

// Atomic operations
await redis.incr('counter');
await redis.expire('counter', 60);
```

#### Go (go-redis)

```go
package main

import (
    "context"
    "fmt"
    "github.com/go-redis/redis/v8"
    "time"
)

func main() {
    // Connect to RustyPotato
    rdb := redis.NewClient(&redis.Options{
        Addr: "localhost:6379",
    })

    ctx := context.Background()

    // Basic operations
    err := rdb.Set(ctx, "mykey", "Hello from Go!", 0).Err()
    if err != nil {
        panic(err)
    }

    val, err := rdb.Get(ctx, "mykey").Result()
    if err != nil {
        panic(err)
    }
    fmt.Println(val) // Hello from Go!

    // Atomic operations
    rdb.Incr(ctx, "counter")
    rdb.Expire(ctx, "counter", time.Minute)
}
```

## ⚙️ Configuration

### Configuration File

Create a `rustypotato.toml` configuration file:

```toml
[server]
port = 6379
bind_address = "0.0.0.0"
max_connections = 10000
tcp_keepalive = true
tcp_nodelay = true

[storage]
# Persistence settings
aof_enabled = true
aof_path = "/data/rustypotato.aof"
aof_fsync = "everysec"  # always, everysec, no
aof_compression = "lz4"  # none, gzip, lz4, zstd

# Memory settings
max_memory = "1GB"
eviction_policy = "lru"  # lru, lfu, random, ttl

[logging]
level = "info"  # trace, debug, info, warn, error
format = "json"  # json, pretty
file = "/var/log/rustypotato.log"
rotation = "daily"  # daily, hourly, size

[metrics]
enabled = true
port = 9090
path = "/metrics"

[security]
# Authentication (bonus feature)
auth_enabled = false
password = "your-secure-password"

# TLS (bonus feature)
tls_enabled = false
cert_file = "/path/to/cert.pem"
key_file = "/path/to/key.pem"

[clustering]
# Distributed mode (unique feature)
enabled = false
node_id = "node-1"
peers = ["node-2:6380", "node-3:6380"]
```

### Environment Variables

Override configuration with environment variables:

```bash
export RUSTYPOTATO_PORT=6380
export RUSTYPOTATO_BIND_ADDRESS="0.0.0.0"
export RUSTYPOTATO_AOF_PATH="/data/rustypotato.aof"
export RUSTYPOTATO_LOG_LEVEL="debug"
export RUSTYPOTATO_MAX_MEMORY="2GB"
```

### Command Line Arguments

```bash
rustypotato-server \
  --port 6380 \
  --bind 0.0.0.0 \
  --aof-path /data/rustypotato.aof \
  --max-connections 20000 \
  --log-level debug \
  --metrics-port 9090
```

## 🐳 Docker

### Official Images

RustyPotato provides multi-architecture Docker images:

```bash
# Latest stable release
docker pull theprantadutta/rustypotato:latest

# Specific version
docker pull theprantadutta/rustypotato:v0.1.0

# Development build
docker pull theprantadutta/rustypotato:dev
```

### Running with Docker

#### Basic Usage

```bash
# Run server with default settings
docker run -d \
  --name rustypotato \
  -p 6379:6379 \
  theprantadutta/rustypotato:latest

# Run with custom configuration
docker run -d \
  --name rustypotato \
  -p 6379:6379 \
  -v $(pwd)/rustypotato.toml:/etc/rustypotato/rustypotato.toml \
  theprantadutta/rustypotato:latest \
  --config /etc/rustypotato/rustypotato.toml
```

#### Persistent Data

```bash
# Create data volume
docker volume create rustypotato-data

# Run with persistent storage
docker run -d \
  --name rustypotato \
  -p 6379:6379 \
  -v rustypotato-data:/data \
  theprantadutta/rustypotato:latest \
  --aof-path /data/rustypotato.aof
```

#### Docker Compose

Create a `docker-compose.yml` file:

```yaml
version: '3.8'

services:
  rustypotato:
    image: theprantadutta/rustypotato:latest
    container_name: rustypotato
    ports:
      - "6379:6379"
      - "9090:9090"  # Metrics port
    volumes:
      - rustypotato-data:/data
      - ./rustypotato.toml:/etc/rustypotato/rustypotato.toml
    environment:
      - RUSTYPOTATO_LOG_LEVEL=info
      - RUSTYPOTATO_METRICS_ENABLED=true
    command: >
      --config /etc/rustypotato/rustypotato.toml
      --aof-path /data/rustypotato.aof
      --metrics-port 9090
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "rustypotato-cli", "ping"]
      interval: 30s
      timeout: 10s
      retries: 3

  # Optional: Prometheus for metrics collection
  prometheus:
    image: prom/prometheus:latest
    ports:
      - "9091:9090"
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml
    depends_on:
      - rustypotato

volumes:
  rustypotato-data:
```

#### Building Custom Images

```dockerfile
# Dockerfile.custom
FROM theprantadutta/rustypotato:latest

# Add custom configuration
COPY custom-config.toml /etc/rustypotato/rustypotato.toml

# Add custom plugins (unique feature)
COPY plugins/*.wasm /etc/rustypotato/plugins/

# Set default command
CMD ["--config", "/etc/rustypotato/rustypotato.toml"]
```

```bash
# Build custom image
docker build -f Dockerfile.custom -t my-rustypotato .

# Run custom image
docker run -d -p 6379:6379 my-rustypotato
```

### Kubernetes Deployment

```yaml
# rustypotato-deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: rustypotato
spec:
  replicas: 3
  selector:
    matchLabels:
      app: rustypotato
  template:
    metadata:
      labels:
        app: rustypotato
    spec:
      containers:
      - name: rustypotato
        image: theprantadutta/rustypotato:latest
        ports:
        - containerPort: 6379
        - containerPort: 9090
        env:
        - name: RUSTYPOTATO_BIND_ADDRESS
          value: "0.0.0.0"
        - name: RUSTYPOTATO_CLUSTERING_ENABLED
          value: "true"
        volumeMounts:
        - name: data
          mountPath: /data
        - name: config
          mountPath: /etc/rustypotato
        livenessProbe:
          exec:
            command:
            - rustypotato-cli
            - ping
          initialDelaySeconds: 30
          periodSeconds: 10
        readinessProbe:
          exec:
            command:
            - rustypotato-cli
            - ping
          initialDelaySeconds: 5
          periodSeconds: 5
      volumes:
      - name: data
        persistentVolumeClaim:
          claimName: rustypotato-pvc
      - name: config
        configMap:
          name: rustypotato-config

---
apiVersion: v1
kind: Service
metadata:
  name: rustypotato-service
spec:
  selector:
    app: rustypotato
  ports:
  - name: redis
    port: 6379
    targetPort: 6379
  - name: metrics
    port: 9090
    targetPort: 9090
  type: LoadBalancer
```

## 🛠️ Development

### Prerequisites

- Rust 1.70+ with Cargo
- Git
- Docker (optional, for integration tests)

### Development Setup

```bash
# Clone repository
git clone https://github.com/theprantadutta/rustypotato.git
cd rustypotato

# Install development dependencies
cargo install cargo-watch cargo-tarpaulin cargo-audit

# Run tests
cargo test

# Run with auto-reload during development
cargo watch -x run

# Check code coverage
cargo tarpaulin --out html

# Security audit
cargo audit
```

### Project Structure

```
rustypotato/
├── src/
│   ├── bin/                 # Binary executables
│   │   ├── server.rs        # Server binary
│   │   └── cli.rs           # CLI client binary
│   ├── commands/            # Command implementations
│   │   ├── string.rs        # SET, GET, DEL, EXISTS
│   │   ├── ttl.rs           # EXPIRE, TTL
│   │   ├── atomic.rs        # INCR, DECR
│   │   └── registry.rs      # Command registry
│   ├── storage/             # Storage layer
│   │   ├── memory.rs        # In-memory storage
│   │   ├── persistence.rs   # AOF persistence
│   │   └── expiration.rs    # TTL management
│   ├── network/             # Network layer
│   │   ├── server.rs        # TCP server
│   │   ├── protocol.rs      # RESP protocol
│   │   └── connection.rs    # Connection handling
│   ├── cli/                 # CLI implementation
│   │   ├── client.rs        # TCP client
│   │   ├── interactive.rs   # REPL mode
│   │   └── commands.rs      # CLI commands
│   ├── metrics/             # Monitoring
│   │   ├── collector.rs     # Metrics collection
│   │   └── server.rs        # HTTP metrics server
│   ├── config.rs            # Configuration management
│   ├── error.rs             # Error types
│   ├── logging.rs           # Logging setup
│   └── lib.rs               # Library root
├── tests/                   # Integration tests
├── benches/                 # Performance benchmarks
├── docs/                    # Documentation
├── docker/                  # Docker configurations
├── .kiro/                   # Kiro specifications
└── scripts/                 # Build and utility scripts
```

### Running Tests

```bash
# Unit tests
cargo test --lib

# Integration tests
cargo test --test '*'

# Specific test module
cargo test storage::memory

# With output
cargo test -- --nocapture

# Performance benchmarks
cargo bench

# Concurrency tests with loom
cargo test --features loom
```

### Code Quality

```bash
# Format code
cargo fmt

# Lint code
cargo clippy -- -D warnings

# Check for security vulnerabilities
cargo audit

# Generate documentation
cargo doc --open

# Profile performance
cargo flamegraph --bin rustypotato-server
```

### Contributing

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/amazing-feature`
3. Make your changes and add tests
4. Ensure all tests pass: `cargo test`
5. Format code: `cargo fmt`
6. Lint code: `cargo clippy`
7. Commit changes: `git commit -m 'Add amazing feature'`
8. Push to branch: `git push origin feature/amazing-feature`
9. Open a Pull Request

## 📊 Performance

### Benchmarks

RustyPotato is designed for high performance with the following characteristics:

| Operation | Throughput | Latency (p99) | Memory Overhead |
|-----------|------------|---------------|-----------------|
| SET | 500K+ ops/sec | < 200μs | ~100 bytes/key |
| GET | 1M+ ops/sec | < 100μs | ~50 bytes/key |
| INCR/DECR | 400K+ ops/sec | < 150μs | ~80 bytes/key |
| DEL | 600K+ ops/sec | < 120μs | N/A |

*Benchmarks run on: Intel i7-12700K, 32GB RAM, NVMe SSD*

### Performance Tuning

#### Server Configuration

```toml
[server]
# Increase connection limit for high-concurrency workloads
max_connections = 50000

# Enable TCP optimizations
tcp_keepalive = true
tcp_nodelay = true

[storage]
# Optimize AOF for write-heavy workloads
aof_fsync = "everysec"  # Balance durability vs performance
aof_compression = "lz4"  # Fast compression

# Configure memory limits
max_memory = "8GB"
eviction_policy = "lru"
```

#### System Tuning

```bash
# Increase file descriptor limits
echo "* soft nofile 65536" >> /etc/security/limits.conf
echo "* hard nofile 65536" >> /etc/security/limits.conf

# Optimize TCP settings
echo "net.core.somaxconn = 65535" >> /etc/sysctl.conf
echo "net.ipv4.tcp_max_syn_backlog = 65535" >> /etc/sysctl.conf

# Apply changes
sysctl -p
```

#### Monitoring Performance

```bash
# Real-time metrics
curl http://localhost:9090/metrics

# Key performance indicators
rustypotato-cli info

# System resource usage
docker stats rustypotato
```

### Load Testing

Use the included benchmark tools:

```bash
# Built-in benchmark
cargo run --bin rustypotato-benchmark -- \
  --address 127.0.0.1:6379 \
  --clients 100 \
  --requests 100000 \
  --pipeline 10

# Redis benchmark compatibility
redis-benchmark -h 127.0.0.1 -p 6379 -n 100000 -c 50
```

## 🤝 Contributing

We welcome contributions! Please see our [Contributing Guide](CONTRIBUTING.md) for details.

### Development Workflow

1. Check existing issues or create a new one
2. Fork the repository and create a feature branch
3. Write code following our [style guide](docs/STYLE_GUIDE.md)
4. Add tests for new functionality
5. Ensure all tests pass and code is formatted
6. Submit a pull request with a clear description

### Code of Conduct

This project follows the [Rust Code of Conduct](https://www.rust-lang.org/policies/code-of-conduct).

## 📄 License

This project is licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## 🙏 Acknowledgments

- The Rust community for excellent tooling and libraries
- Redis for inspiring the protocol and API design
- All contributors who help make RustyPotato better

---

**Ready to get started?** [Install RustyPotato](#-installation) and join our community!

For questions, issues, or discussions, visit our [GitHub repository](https://github.com/theprantadutta/rustypotato).