# Changelog

All notable changes to RustyPotato are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Pre-1.0, breaking changes can land in any minor release; we'll always
call them out explicitly here.

## [0.2.0] — 2026-05-05

The first release where the README's claims actually match the
binary's behaviour. v0.1.0 had several silently-broken core paths
(persistence wasn't wired in the bin, hash values were lost on
restart, server shutdown was a no-op, RESP parser could OOM the server
with one client message). This release closes those and lands the
foundational refactors needed to build on top of safely.

### Breaking changes

- `Command::execute` now takes `args: &[bytes::Bytes]` instead of
  `&[String]`. Out-of-tree command implementations need to migrate.
- `ResponseValue::BulkString(Option<Bytes>)` (was `Option<String>`).
- `ValueType::String(Bytes)` (was `String`).
- `RespValue::BulkString(Option<Bytes>)` (was `Option<String>`).
- `MemoryStore::get` is now `&self` (was `&mut self`-ish via internal
  `get_mut`); read paths no longer serialize on the per-shard write
  lock.
- Removed `StorageConfig::memory_limit` and
  `NetworkConfig::connection_timeout` — both fields were parsed and
  validated but never enforced. Setting them in a config file is now
  ignored.
- Dropped public types that were dead infrastructure:
  `LoggingSystem`, `ExpirationManager`, the duplicate `MetricsServer`
  in the metrics module, `BufferPool`, `MemoryStats`,
  `ConnectionPoolStats`, `ConnectionInfo`. Plus several admin getters
  on `ConnectionPool` and `ClientConnection` that nothing called.

### Added — Redis 7 command surface

Registered command count: 33.

- **String / generic key:** `MGET`, `MSET`, plus full `SET` option set
  (`NX`, `XX`, `EX`, `PX`, `EXAT`, `PXAT`, `KEEPTTL`, `GET`).
- **TTL:** `PTTL`, `PEXPIRE`, `EXPIREAT`, `PEXPIREAT`. `EXPIRE`-family
  flags: `NX`, `XX`, `GT`, `LT`.
- **Hash:** variadic `HSET key field val [field val ...]`, plus
  `HMGET`, `HKEYS`, `HVALS`, `HLEN`.
- **Server:** `PING`, `ECHO`, `COMMAND`, `INFO`, `DBSIZE`, `TYPE`,
  `FLUSHDB`, `KEYS` (glob with `*`/`?`/`[abc]`/`\` escapes), `QUIT`,
  `BGREWRITEAOF`.

### Added — durability and lifecycle

- `BGREWRITEAOF` for AOF compaction with a rewrite barrier — new
  writes during a rewrite are merged into the new file before the
  atomic swap.
- Real graceful shutdown with connection drain (was a no-op in v0.1).
- Persistence is now actually wired in `bin/server.rs` — the server
  loads / saves the AOF (was parsed-but-discarded in v0.1).
- AOF is now RESP-frame format. Recovery replays through the command
  dispatch path; any new command added in the future is automatically
  AOF-safe.

### Added — DoS hardening

- Bounded RESP codec: `max_bulk_size`, `max_array_length`,
  `max_buffer_size` configurable. A single client can no longer OOM
  the server via `*<i32>\r\n`.
- Semaphore-based `max_connections` (no TOCTOU race).
- Idle-connection eviction with configurable `idle_timeout`.
- `SO_KEEPALIVE` actually applied via socket2 (was a stub in v0.1).

### Added — observability

- Prometheus histogram percentile uses linear interpolation. v0.1
  reported the bucket upper bound, which inflated p50/p95/p99.
- Metrics command-name cardinality is bounded — sending random
  command names no longer fills the latency map.
- `LogRotationManager` status reads the same `Arc` the writer task
  updates (was always stale in v0.1).
- HTTP monitoring server binds to `bind_address`, not hardcoded
  `127.0.0.1` (containers can now scrape `/metrics`).

### Added — tooling

- `tools/network_bench` measures end-to-end RESP-over-TCP throughput
  and latency with 50 concurrent clients and pipeline depth 16. README
  perf numbers come from this.
- `tools/storage_microbench` (renamed from `quick_bench`) is now
  clearly labelled as in-process-only.
- `rustypotato-cli interactive` uses rustyline: persistent history at
  `~/.rustypotato_history`, tab completion for command names.

### Fixed — latent correctness bugs

- `bin/init_logging` honors `config.logging.format` — `Json` no longer
  silently produces `Pretty`.
- CLI codec's scratch buffer is cleared on encode exit; was leaking
  request bytes into the response parser, so `cli ping` rendered
  `1) PING` instead of `PONG`.
- Dockerfile copies `tools/` (the Cargo manifest references entries
  there).
- `docker-compose.yml` flag set matches the actual server CLI; port
  mapping matches the real monitoring port (`port + 1000`).
- Healthcheck uses the new `rustypotato-cli ping` subcommand.

### Removed — dead infrastructure

Roughly 9,000 lines of code removed across:
- `LoggingSystem` and the parallel logging stack
- `ExpirationManager` (background sweeper was never started)
- The duplicate `MetricsServer` in `metrics/`
- `BufferPool`, fake `memory_stats`, dead config flags
- Orphaned tests and admin methods that nothing called

### CI / release

- `actions/cache@v4`, `codecov-action@v4`,
  `softprops/action-gh-release@v2`. Replaced unmaintained
  `actions-rs/audit-check@v1` with `cargo-audit`. Dropped the flaky
  nightly toolchain row.

### Tests

365 lib + 220 integration tests, all green. CI gate is
`cargo check --all-targets && cargo clippy --all-targets -- -D warnings && cargo test`.

---

## [0.1.0] — 2025-08-05

Initial public release. Marketed feature set was largely correct on
paper but several subsystems were silently broken at runtime — see the
v0.2.0 "Fixed" section.

### Added

- In-memory key-value store with DashMap.
- Redis RESP2 protocol over TCP.
- Commands: `SET`, `GET`, `DEL`, `EXISTS`, `EXPIRE`, `TTL`, `INCR`,
  `DECR`, `HSET`, `HGET`, `HDEL`, `HGETALL`, `HEXISTS`.
- AOF persistence (text-format; replaced in v0.2).
- TCP server with configurable connection limits.
- CLI client (basic; replaced with rustyline-based REPL in v0.2).
- TOML configuration with environment variable overrides.
- Prometheus metrics endpoint.
- Multi-arch Docker images.
- Cross-platform release binaries (Linux x64/arm64, macOS x64/arm64,
  Windows x64).

---

## Roadmap

These are not commitments — they're the order things are likely to
land in if no one shouts louder for something else. File an issue if
you need any of them and the priority shifts.

### Next (probably v0.3.0)

- List type: `LPUSH`, `RPUSH`, `LPOP`, `RPOP`, `LRANGE`, `LLEN`.
- Set type: `SADD`, `SREM`, `SMEMBERS`, `SCARD`, `SISMEMBER`.
- Pub/sub: `SUBSCRIBE`, `PSUBSCRIBE`, `PUBLISH`, `UNSUBSCRIBE`.
- Auth: `AUTH` command + `requirepass` config.

### Later

- LRU eviction with `memory_limit` config (sketched in
  `plans/refactor.md` Stage 10).
- TLS termination (or a documented sidecar pattern).
- Transactions: `MULTI`, `EXEC`, `DISCARD`, `WATCH`.
- Sorted sets.
- Streams.
- Replication (single-leader async).

### Maybe / aspirational

Cluster mode, scripting (`EVAL`), JSON values with dot-notation
queries, WASM plugin hooks. These are interesting but expensive; only
worth starting once the core is solid.

---

## Migration notes

### From v0.1.0 to v0.2.0

- **AOF format changed** from text to RESP frames. The new server
  cannot replay an old AOF. To migrate: bring up the v0.1 server, dump
  the keyspace via `KEYS '*'` + `DUMP` from a client, shut it down,
  start v0.2 with a fresh AOF path, and replay the dump. If your data
  fits in memory and you can take a brief outage, this is a few lines
  of Python.
- **Out-of-tree command implementations** need to migrate from
  `&[String]` to `&[Bytes]` — see the `Command` trait in
  `src/commands/registry.rs`.
- **Config files** with `storage.memory_limit`,
  `network.connection_timeout`, `security.*`, or `clustering.*` will
  now silently ignore those fields. The server will start as if
  they're absent.

### From Redis

RustyPotato is wire-compatible for the [supported
commands](docs/COMMANDS.md). Existing Redis clients work without
modification. The compatibility gaps are listed in
[docs/CLIENTS.md](docs/CLIENTS.md#known-compatibility-gaps).
