# Configuration

RustyPotato reads configuration from three sources, applied in order
(later sources win):

1. Built-in defaults
2. A TOML config file
3. Environment variables prefixed with `RUSTYPOTATO_`
4. Command-line flags (server binary only)

## Config file location

The server looks for a TOML file in the first location that exists:

1. `./rustypotato.toml` (current directory)
2. `./config/rustypotato.toml`
3. `/etc/rustypotato/rustypotato.toml`
4. `$XDG_CONFIG_HOME/rustypotato/rustypotato.toml` (or platform equivalent)

Or pass a path explicitly: `rustypotato-server --config /my/path.toml`.

## Full reference

```toml
[server]
# TCP port the RESP listener binds to.
port = 6379
# Address to bind. Use 0.0.0.0 to accept connections from any interface.
bind_address = "127.0.0.1"
# Hard cap on simultaneous client connections. Enforced by a semaphore;
# the (max_connections + 1)th connection is rejected at accept time.
max_connections = 10000
# Tokio worker thread count. Default (omit / null) = number of CPUs.
# worker_threads = 4

[storage]
# Master switch for AOF persistence. When false, the store is purely
# in-memory and a restart loses all data.
aof_enabled = true
# Path to the AOF file. Relative paths are relative to the server's
# working directory.
aof_path = "rustypotato.aof"
# When to fsync the AOF:
#   "Always"      — fsync after every write (slowest, strongest durability)
#   "EverySecond" — fsync once per second (default; ~1s data loss window)
#   "Never"       — let the OS decide (fastest, weakest durability)
aof_fsync_policy = "EverySecond"

[network]
# Disable Nagle's algorithm. Lower latency for small commands, slightly
# more packet overhead. Default true matches Redis.
tcp_nodelay = true
# Enable SO_KEEPALIVE on accepted connections. Lets the kernel detect
# dead peers (NAT timeout, network failure) without an application
# heartbeat.
tcp_keepalive = true
# Per-read deadline in seconds. A client that opens a connection but
# sends nothing for this long has the read side time out.
read_timeout = 30
# Per-write deadline in seconds.
write_timeout = 30
# Maximum size in bytes of a single RESP bulk string. Matches Redis'
# `proto-max-bulk-len` default of 512 MB. Larger inputs get a protocol
# error and the connection closes — protects against a single client
# OOMing the server with `*<huge-int>\r\n`.
max_bulk_size = 536870912
# Maximum number of elements in a single RESP array (e.g. variadic
# MSET).
max_array_length = 1048576
# Maximum bytes a single connection's protocol-decode buffer is
# allowed to grow to before the connection is dropped. Defends against
# slow-loris and clients trickling junk in.
max_buffer_size = 67108864
# Per-connection idle timeout in seconds. After this much wall-clock
# time without any client activity, the connection is closed by the
# background eviction task.
idle_timeout = 300

[logging]
# trace, debug, info, warn, error
level = "info"
# Pretty (human-readable, colored), Json (one event per line, structured),
# or Compact (single line, no decoration).
format = "Pretty"
# Optional log file path. If omitted, logs go to stderr only.
# file_path = "/var/log/rustypotato/rustypotato.log"
```

## Environment variables

Any field above can be overridden via env vars by joining its path with
`_` and prefixing `RUSTYPOTATO_`:

```bash
export RUSTYPOTATO_SERVER_PORT=6380
export RUSTYPOTATO_SERVER_BIND_ADDRESS="0.0.0.0"
export RUSTYPOTATO_STORAGE_AOF_PATH="/data/rustypotato.aof"
export RUSTYPOTATO_STORAGE_AOF_FSYNC_POLICY="Always"
export RUSTYPOTATO_LOGGING_LEVEL="debug"
export RUSTYPOTATO_LOGGING_FORMAT="Json"
export RUSTYPOTATO_NETWORK_IDLE_TIMEOUT=60
```

## Server command-line flags

Only a handful of flags are wired on the binary; everything else lives
in the config file or env vars.

```
rustypotato-server [OPTIONS]

  -c, --config FILE      Configuration file path
  -p, --port PORT        Override server.port
  -b, --bind ADDRESS     Override server.bind_address
  -l, --log-level LEVEL  Override logging.level (trace|debug|info|warn|error)
  -V, --version          Print version and exit
  -h, --help             Print help
```

## Monitoring port

When the server starts, an HTTP monitoring endpoint is bound at
`bind_address:(port + 1000)`. With the defaults that's
`127.0.0.1:7379`. It serves three routes:

- `GET /health` — liveness probe (200 if up).
- `GET /metrics` — Prometheus exposition format.
- `GET /info` — JSON build/runtime info.

The port is intentionally not configurable in v0.2.0 to keep deployment
simple. If you bind the main port to `0.0.0.0`, the monitoring port
binds to `0.0.0.0` too — see your firewall rules.

## CLI client flags

The `rustypotato-cli` binary speaks RESP to a running server.

```
rustypotato-cli [-a 127.0.0.1:6379] <subcommand>

Subcommands:
  ping
  set <key> <value>
  get <key>
  del <key>
  exists <key>
  expire <key> <seconds>
  ttl <key>
  interactive       Open a REPL with line editing and persistent history
```

For the interactive REPL: command history is persisted to
`~/.rustypotato_history`, and `<TAB>` completes registered command
names. Inside the REPL you can call any of the commands documented in
[COMMANDS.md](COMMANDS.md), not just the few subcommands above.

## Knobs that used to exist

The following fields appeared in older versions of this file. They were
parsed but never enforced — printed in the startup banner and silently
discarded. They have been removed; setting them in a config file is
ignored:

- `storage.memory_limit` — There is no LRU eviction loop yet. To bound
  resident memory, run RustyPotato under a cgroup memory limit, a
  Kubernetes `resources.limits.memory`, or a systemd `MemoryMax=`, and
  let the kernel OOM-kill it when it exceeds the bound. The eviction
  sampler design is sketched in `plans/refactor.md` Stage 10.
- `storage.eviction_policy` — Same story; will return when LRU lands.
- `network.connection_timeout` — Connection lifetime is governed by
  `read_timeout`, `write_timeout`, and `idle_timeout` instead.
- `security.auth_enabled` / `security.password` — Authentication
  isn't implemented in v0.2.0. Front the server with a network ACL
  (firewall, security group, mesh policy) for now.
- `security.tls_*` — TLS isn't implemented either. Use a sidecar
  (stunnel, Envoy, nginx-stream) or a service mesh.
- `clustering.*` — RustyPotato is single-node in v0.2.0. Cluster mode
  is on the roadmap but not started.
