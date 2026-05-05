# RustyPotato 🥔

[![CI](https://github.com/theprantadutta/rustypotato/actions/workflows/ci.yml/badge.svg)](https://github.com/theprantadutta/rustypotato/actions/workflows/ci.yml)
[![Release](https://github.com/theprantadutta/rustypotato/actions/workflows/release.yml/badge.svg)](https://github.com/theprantadutta/rustypotato/actions/workflows/release.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)

A Redis-compatible key-value store, written in Rust, single-node,
binary-safe, with durable AOF persistence.

> ⚠️ **Pre-production.** RustyPotato compiles, has a green test suite,
> and speaks RESP correctly enough for real Redis clients to drive it.
> But it's young: there's no replication, no clustering, no auth, and
> no TLS. Don't put it in front of customer traffic. Do experiment with
> it, run it as a cache in dev, and file issues when you find sharp
> edges.

## What works in v0.2.0

- 33 Redis commands across strings, hashes, atomic counters, TTL, and
  server admin — see [docs/COMMANDS.md](docs/COMMANDS.md).
- RESP2 protocol, binary-safe (`SET foo $'\xff\xfe\xfd'` round-trips
  cleanly).
- DashMap-backed in-memory store with lock-free reads.
- AOF persistence with `Always` / `EverySecond` / `Never` fsync
  policies. `BGREWRITEAOF` for compaction.
- Bounded RESP codec, idle-connection eviction, semaphore-bounded
  `max_connections`. A single client can't OOM the server.
- Prometheus `/metrics` endpoint with real percentile interpolation,
  bounded command-name cardinality.
- Multi-arch Docker images, cross-platform release binaries, systemd
  service file, Grafana dashboard, Compose stack with Prometheus +
  Grafana wired up.

## What doesn't work yet

Lists, sets, sorted sets, pub/sub, transactions, scripting, auth, TLS,
multi-database, clustering. See
[docs/COMMANDS.md](docs/COMMANDS.md#whats-not-yet-implemented).

## Quick start

### Docker

```bash
docker run -d -p 6379:6379 -p 7379:7379 \
  --name rustypotato \
  rustypotato/rustypotato:latest \
  rustypotato-server --bind 0.0.0.0

redis-cli -h 127.0.0.1 -p 6379 ping       # PONG
```

Port 7379 is the HTTP monitoring endpoint (`/health`, `/metrics`,
`/info`) — see [docs/CONFIGURATION.md](docs/CONFIGURATION.md#monitoring-port).

### Pre-built binaries

Grab one from [GitHub
Releases](https://github.com/theprantadutta/rustypotato/releases) for
your platform:

```bash
curl -L https://github.com/theprantadutta/rustypotato/releases/latest/download/rustypotato-linux-x64.tar.gz | tar xz
cd rustypotato-linux-x64
./rustypotato-server &
./rustypotato-cli ping        # PONG
```

Available archives:
`rustypotato-linux-x64.tar.gz`,
`rustypotato-linux-arm64.tar.gz`,
`rustypotato-macos-x64.tar.gz`,
`rustypotato-macos-arm64.tar.gz`,
`rustypotato-windows-x64.zip`.

### From source

```bash
git clone https://github.com/theprantadutta/rustypotato.git
cd rustypotato
cargo build --release
./target/release/rustypotato-server
```

Requires Rust 1.70+.

## First commands

Use any Redis client. From the command line:

```bash
redis-cli set greeting "hello world" EX 60
redis-cli get greeting                    # "hello world"
redis-cli hset user:1 name ada role admin
redis-cli hgetall user:1
redis-cli incr counter
redis-cli incr counter
redis-cli get counter                     # "2"
```

Or use the bundled CLI for an interactive REPL with persistent history
and tab completion:

```bash
rustypotato-cli interactive
> SET foo bar
+OK
> GET foo
"bar"
```

For client library examples (Python, Node, Go, Rust), see
[docs/CLIENTS.md](docs/CLIENTS.md).

## Configuration

Defaults are sensible. To customize, drop a TOML file at
`./rustypotato.toml`, `/etc/rustypotato/rustypotato.toml`, or pass
`--config <path>`:

```toml
[server]
port = 6379
bind_address = "0.0.0.0"
max_connections = 10000

[storage]
aof_enabled = true
aof_path = "/var/lib/rustypotato/rustypotato.aof"
aof_fsync_policy = "EverySecond"

[logging]
level = "info"
format = "Json"
```

Full reference (every knob, every env var, every CLI flag) lives at
[docs/CONFIGURATION.md](docs/CONFIGURATION.md).

## Performance

There are three benchmarks in this repo and they measure different
things — see [docs/BENCHMARKING.md](docs/BENCHMARKING.md). The short
version: run `tools/network_bench.rs` on your own hardware and quote
those numbers, not in-process microbench numbers.

```bash
./target/release/rustypotato-server &
cargo run --release --bin network_bench
```

This benchmark sets up 50 concurrent clients with pipeline depth 16 and
hammers SET/GET/INCR over real TCP+RESP. The output is directly
comparable to `redis-benchmark`.

## Documentation

- **[docs/COMMANDS.md](docs/COMMANDS.md)** — Full command reference,
  options, error semantics.
- **[docs/CONFIGURATION.md](docs/CONFIGURATION.md)** — Every config
  option with defaults, env vars, CLI flags.
- **[docs/CLIENTS.md](docs/CLIENTS.md)** — Connecting from Python,
  Node, Go, Rust, redis-cli; known compatibility gaps.
- **[docs/BENCHMARKING.md](docs/BENCHMARKING.md)** — How to measure
  performance honestly.
- **[docs/DEPLOYMENT.md](docs/DEPLOYMENT.md)** — Docker, Kubernetes,
  bare metal, persistence, monitoring, backups.
- **[CHANGELOG.md](CHANGELOG.md)** — What changed between releases.
- **[CONTRIBUTING.md](CONTRIBUTING.md)** — Development setup, coding
  standards, PR process.

## Development

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

365 lib tests, 220 integration tests. The CI gate is the same four
commands above.

To start a server with auto-reload during development:

```bash
cargo install cargo-watch
cargo watch -x 'run --bin rustypotato-server'
```

## Project layout

```
rustypotato/
├── src/
│   ├── bin/             # rustypotato-server, rustypotato-cli
│   ├── commands/        # 33 commands across string/hash/atomic/ttl/server
│   ├── storage/         # MemoryStore + AOF persistence
│   ├── network/         # TCP server + RESP codec
│   ├── cli/             # CLI client + interactive REPL
│   ├── monitoring/      # /health, /metrics, /info HTTP server
│   └── metrics/         # Prometheus metrics collection
├── tests/               # Integration tests
├── benches/             # Criterion benchmarks
├── tools/
│   ├── network_bench.rs    # End-to-end RESP-over-TCP benchmark
│   └── storage_microbench.rs # In-process storage ceiling
├── docker/              # Compose config, Prometheus rules, Grafana dashboard
└── docs/                # Documentation
```

## License

Dual-licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Acknowledgments

- The Rust community for the tooling that makes this kind of project
  tractable.
- Redis for the protocol and command surface.
