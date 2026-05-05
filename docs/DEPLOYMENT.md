# Deployment

This guide covers running RustyPotato in real environments. It is
deliberately conservative: it documents what works in v0.2.0, not
what's planned. For configuration knobs, see
[CONFIGURATION.md](CONFIGURATION.md).

## Contents

- [Quick start](#quick-start)
- [Docker](#docker)
- [Kubernetes](#kubernetes)
- [Bare metal / systemd](#bare-metal--systemd)
- [Persistence](#persistence)
- [Monitoring](#monitoring)
- [Backup and recovery](#backup-and-recovery)
- [Network security](#network-security)
- [Troubleshooting](#troubleshooting)

---

## Quick start

Pre-built binaries from [GitHub
Releases](https://github.com/theprantadutta/rustypotato/releases):

```bash
# Linux x64
curl -L https://github.com/theprantadutta/rustypotato/releases/latest/download/rustypotato-linux-x64.tar.gz | tar xz
cd rustypotato-linux-x64
./rustypotato-server &
./rustypotato-cli ping       # PONG
```

Available archives:

| Platform | Archive |
|---|---|
| Linux x86_64 | `rustypotato-linux-x64.tar.gz` |
| Linux aarch64 | `rustypotato-linux-arm64.tar.gz` |
| macOS x86_64 | `rustypotato-macos-x64.tar.gz` |
| macOS aarch64 | `rustypotato-macos-arm64.tar.gz` |
| Windows x86_64 | `rustypotato-windows-x64.zip` |

Each archive ships with a `.sha256` companion file for verification.

---

## Docker

### Single container

```bash
docker run -d \
  --name rustypotato \
  -p 6379:6379 \
  -p 7379:7379 \
  -v rustypotato-data:/data \
  rustypotato/rustypotato:latest \
  rustypotato-server --bind 0.0.0.0
```

Two ports because RustyPotato exposes a separate monitoring HTTP server
at `port + 1000`. With the default RESP port at 6379, monitoring lives
at 7379. See [Monitoring](#monitoring).

### Docker Compose

The repository ships a working `docker-compose.yml` with the server,
Prometheus, and Grafana wired together. From a checkout:

```bash
docker-compose up -d
```

Services:

- `rustypotato` — the server, exposing 6379 (RESP) and 7379 (HTTP monitoring)
- `prometheus` — scrapes `/metrics` every interval defined in
  `docker/prometheus.yml`, exposed on host port 9091
- `grafana` — pre-provisioned with the rustypotato dashboard, exposed on
  host port 3000 (login `admin` / `admin`)

The included `docker/rustypotato.toml` is a minimal config tuned for
container deployment. Override it by mounting your own at the same path
or pointing `--config` somewhere else.

### Persistence

Mount a volume at `/data` and point the AOF at it via the config file:

```toml
# docker/rustypotato.toml
[storage]
aof_enabled = true
aof_path = "/data/rustypotato.aof"
aof_fsync_policy = "EverySecond"
```

Compose example:

```yaml
services:
  rustypotato:
    image: rustypotato/rustypotato:latest
    volumes:
      - rustypotato-data:/data
      - ./docker/rustypotato.toml:/etc/rustypotato/rustypotato.toml:ro
    command: rustypotato-server --config /etc/rustypotato/rustypotato.toml --bind 0.0.0.0
    ports:
      - "6379:6379"
      - "7379:7379"
volumes:
  rustypotato-data:
```

### Healthchecks

```yaml
healthcheck:
  test: ["CMD", "rustypotato-cli", "ping"]
  interval: 30s
  timeout: 10s
  retries: 3
  start_period: 10s
```

Or hit the HTTP `/health` endpoint:

```yaml
healthcheck:
  test: ["CMD", "curl", "-f", "http://localhost:7379/health"]
```

---

## Kubernetes

A `StatefulSet` is the right shape — RustyPotato has stable on-disk
state in the form of the AOF, and a single instance per replica.

### Minimal StatefulSet

```yaml
apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: rustypotato
spec:
  serviceName: rustypotato-headless
  replicas: 1
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
        image: rustypotato/rustypotato:latest
        args:
          - rustypotato-server
          - --bind
          - "0.0.0.0"
          - --config
          - /etc/rustypotato/rustypotato.toml
        ports:
        - containerPort: 6379
          name: resp
        - containerPort: 7379
          name: metrics
        volumeMounts:
        - name: data
          mountPath: /data
        - name: config
          mountPath: /etc/rustypotato
        resources:
          requests:
            memory: "1Gi"
            cpu: "500m"
          limits:
            memory: "4Gi"
            cpu: "2000m"
        livenessProbe:
          exec:
            command: ["rustypotato-cli", "ping"]
          initialDelaySeconds: 30
          periodSeconds: 10
        readinessProbe:
          httpGet:
            path: /health
            port: 7379
          initialDelaySeconds: 5
          periodSeconds: 5
      volumes:
      - name: config
        configMap:
          name: rustypotato-config
  volumeClaimTemplates:
  - metadata:
      name: data
    spec:
      accessModes: ["ReadWriteOnce"]
      storageClassName: standard
      resources:
        requests:
          storage: 20Gi
```

### Service

```yaml
apiVersion: v1
kind: Service
metadata:
  name: rustypotato
spec:
  selector:
    app: rustypotato
  ports:
  - name: resp
    port: 6379
    targetPort: resp
  - name: metrics
    port: 7379
    targetPort: metrics
```

### Prometheus scraping

If you run the Prometheus operator, a `ServiceMonitor` does it:

```yaml
apiVersion: monitoring.coreos.com/v1
kind: ServiceMonitor
metadata:
  name: rustypotato
spec:
  selector:
    matchLabels:
      app: rustypotato
  endpoints:
  - port: metrics
    interval: 30s
    path: /metrics
```

### Important: replicas = 1

RustyPotato v0.2.0 is single-node. Setting `replicas > 1` does **not**
give you replication or sharding — it gives you N independent stores
that don't talk to each other. If you need HA, run multiple isolated
instances and manage failover at the application level until clustering
ships.

---

## Bare metal / systemd

```bash
# Create user and dirs
sudo useradd -r -s /bin/false rustypotato
sudo mkdir -p /etc/rustypotato /var/lib/rustypotato /var/log/rustypotato
sudo chown rustypotato:rustypotato /var/lib/rustypotato /var/log/rustypotato

# Install binaries
curl -L https://github.com/theprantadutta/rustypotato/releases/latest/download/rustypotato-linux-x64.tar.gz | tar xz
sudo install -m 755 rustypotato-linux-x64/rustypotato-server /usr/local/bin/
sudo install -m 755 rustypotato-linux-x64/rustypotato-cli /usr/local/bin/

# Config
sudo tee /etc/rustypotato/rustypotato.toml > /dev/null <<'EOF'
[server]
port = 6379
bind_address = "127.0.0.1"
max_connections = 10000

[storage]
aof_enabled = true
aof_path = "/var/lib/rustypotato/rustypotato.aof"
aof_fsync_policy = "EverySecond"

[logging]
level = "info"
format = "Json"
file_path = "/var/log/rustypotato/rustypotato.log"
EOF

# systemd unit
sudo tee /etc/systemd/system/rustypotato.service > /dev/null <<'EOF'
[Unit]
Description=RustyPotato Key-Value Store
After=network.target

[Service]
Type=simple
User=rustypotato
Group=rustypotato
ExecStart=/usr/local/bin/rustypotato-server --config /etc/rustypotato/rustypotato.toml
Restart=always
RestartSec=5
LimitNOFILE=65536

# Resource bounds — RustyPotato has no internal memory limit, so the
# kernel needs to be the backstop. Tune to your workload.
MemoryMax=2G

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable --now rustypotato
sudo systemctl status rustypotato
```

### Kernel/file-descriptor tuning

For high-connection-count workloads:

```bash
# /etc/sysctl.d/99-rustypotato.conf
net.core.somaxconn = 65535
net.ipv4.tcp_max_syn_backlog = 65535
net.ipv4.tcp_fin_timeout = 30
vm.overcommit_memory = 1
vm.swappiness = 1

# Apply
sudo sysctl -p /etc/sysctl.d/99-rustypotato.conf
```

```bash
# /etc/security/limits.d/99-rustypotato.conf
rustypotato soft nofile 65536
rustypotato hard nofile 65536
```

---

## Persistence

RustyPotato uses an AOF (append-only file) for durability. Every
mutating command is RESP-framed and appended to the file at
`storage.aof_path`; on startup, the file is replayed through the
command dispatch path to rebuild the in-memory state.

### Fsync policy

| Policy | Durability | Throughput |
|---|---|---|
| `Always` | Strongest. fsync after every write. | Slowest. |
| `EverySecond` (default) | ~1 second of data may be lost on crash. | Fast. |
| `Never` | OS decides. May lose minutes of data on crash. | Fastest. |

`EverySecond` is the right default for almost everyone. Pick `Always`
only if a one-second loss window is genuinely unacceptable, and
benchmark before you do — it changes write throughput significantly.

### Compaction (BGREWRITEAOF)

The AOF grows unboundedly otherwise. Run `BGREWRITEAOF` periodically to
compact it:

```bash
rustypotato-cli interactive
> BGREWRITEAOF
```

During the rewrite, new mutations are buffered and merged in at the
end. The old file is replaced atomically when the rewrite completes.

There is no automatic trigger in v0.2.0 — schedule it yourself via
cron / a Kubernetes CronJob / a systemd timer.

### Recovery

On startup, if `aof_enabled = true` and the AOF file exists,
RustyPotato replays it before accepting connections. During replay the
server returns `LOADING ...` errors to any incoming command. Replay is
typically much faster than the original write rate because there's no
network or fsync overhead per command.

---

## Monitoring

The server starts an HTTP monitoring server at
`bind_address:(port + 1000)`. Three routes:

- `GET /health` — 200 if the server is up. Use as a liveness probe.
- `GET /metrics` — Prometheus exposition format. Use as a scrape target.
- `GET /info` — JSON build/runtime info (version, uptime, OS, arch).

The monitoring server binds to the same address as the main RESP
server, so if `bind_address = "0.0.0.0"` your `/metrics` endpoint is
also on `0.0.0.0`. Restrict it with a firewall or by running RustyPotato
behind a network ACL.

### Useful Prometheus metrics

- `rustypotato_commands_total{command="GET",status="ok"}` — total
  commands executed, broken down by name and status
- `rustypotato_command_duration_seconds_*` — histogram of command
  latencies (use `histogram_quantile` to derive p50/p95/p99)
- `rustypotato_connections_active` — current connection count
- `rustypotato_aof_writes_total` — AOF mutation count
- `rustypotato_aof_fsync_duration_seconds_*` — fsync histogram

The repo ships a Grafana dashboard at
`docker/grafana/dashboards/rustypotato.json` — import it and point it
at your Prometheus.

---

## Backup and recovery

The AOF file is the entire on-disk state. Backups are file copies:

```bash
#!/bin/bash
# /usr/local/bin/rustypotato-backup
set -euo pipefail
BACKUP_DIR=/var/backups/rustypotato
AOF=/var/lib/rustypotato/rustypotato.aof
DATE=$(date +%Y%m%d-%H%M%S)

mkdir -p "$BACKUP_DIR"
cp "$AOF" "$BACKUP_DIR/rustypotato-$DATE.aof"
gzip "$BACKUP_DIR/rustypotato-$DATE.aof"

# Retain 30 days
find "$BACKUP_DIR" -name 'rustypotato-*.aof.gz' -mtime +30 -delete
```

Run it from cron or a systemd timer. Copying a live AOF is safe: the
file is append-only, so the worst case is a backup that's missing the
final partial entry, which the AOF replay logic handles gracefully on
restore.

To restore:

```bash
sudo systemctl stop rustypotato
gunzip -c /var/backups/rustypotato/rustypotato-20260101-120000.aof.gz \
  > /var/lib/rustypotato/rustypotato.aof
sudo chown rustypotato:rustypotato /var/lib/rustypotato/rustypotato.aof
sudo systemctl start rustypotato
```

---

## Network security

RustyPotato has **no built-in authentication or TLS** in v0.2.0. Don't
expose the RESP port to untrusted networks. Use:

- A firewall rule limiting source IPs
- A private subnet / VPC
- A service mesh (Istio, Linkerd) for mTLS
- A TLS-terminating sidecar (stunnel, Envoy, nginx-stream)

Bare iptables example for a single trusted CIDR:

```bash
sudo iptables -A INPUT -p tcp --dport 6379 -s 10.0.0.0/8 -j ACCEPT
sudo iptables -A INPUT -p tcp --dport 6379 -j DROP

sudo iptables -A INPUT -p tcp --dport 7379 -s 10.0.0.0/8 -j ACCEPT
sudo iptables -A INPUT -p tcp --dport 7379 -j DROP
```

---

## Troubleshooting

### Connection refused

```bash
# Is the process running?
systemctl status rustypotato       # systemd
docker ps | grep rustypotato       # docker

# Is the port listening?
ss -tlnp | grep 6379               # Linux
netstat -ano | findstr :6379       # Windows

# Firewall?
sudo iptables -L -n | grep 6379
```

### `LOADING` errors on startup

The server is replaying its AOF. This is expected and bounded — wait
for it to finish. If replay takes too long, consider running
`BGREWRITEAOF` periodically to keep the AOF compact.

### High memory usage

RustyPotato has no internal memory cap in v0.2.0. To bound resident
memory:

- **systemd:** `MemoryMax=2G` in the unit file.
- **Kubernetes:** `resources.limits.memory: 2Gi`.
- **Docker:** `--memory 2g` on `docker run` or `mem_limit: 2g` in
  Compose.

The kernel will OOM-kill the process when it crosses the bound. The
AOF is durable, so on restart the state recovers.

### Slow GETs

Run `cargo run --release --bin network_bench` (or `redis-benchmark`)
to confirm whether it's a server-side issue or a client/network issue.
See [BENCHMARKING.md](BENCHMARKING.md).

### AOF file too large

```bash
rustypotato-cli interactive
> BGREWRITEAOF
```

This compacts the file in-place. There's no downtime.

### Logs

```bash
# systemd
journalctl -u rustypotato -f

# docker
docker logs -f rustypotato

# kubernetes
kubectl logs -f statefulset/rustypotato
```

For more verbosity, restart with `--log-level debug` or set
`RUSTYPOTATO_LOGGING_LEVEL=debug`.
