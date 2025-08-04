# RustyPotato Deployment Guide

This guide covers various deployment options for RustyPotato, from development to production environments.

## Table of Contents

- [Quick Start](#quick-start)
- [Docker Deployment](#docker-deployment)
- [Kubernetes Deployment](#kubernetes-deployment)
- [Bare Metal Installation](#bare-metal-installation)
- [Cloud Deployments](#cloud-deployments)
- [Configuration](#configuration)
- [Monitoring and Observability](#monitoring-and-observability)
- [Security](#security)
- [Performance Tuning](#performance-tuning)
- [Backup and Recovery](#backup-and-recovery)
- [Troubleshooting](#troubleshooting)

## Quick Start

### Docker (Recommended for Development)

```bash
# Run RustyPotato with default settings
docker run -d -p 6379:6379 --name rustypotato theprantadutta/rustypotato:latest

# Test the connection
docker run --rm --link rustypotato theprantadutta/rustypotato:latest rustypotato-cli ping
```

### Pre-built Binaries

```bash
# Download and extract
curl -L https://github.com/theprantadutta/rustypotato/releases/latest/download/rustypotato-linux-x64.tar.gz | tar xz

# Run server
./rustypotato-server

# Test with CLI
./rustypotato-cli ping
```

## Docker Deployment

### Basic Docker Setup

#### Single Container

```bash
# Create data volume
docker volume create rustypotato-data

# Run with persistent storage
docker run -d \
  --name rustypotato \
  -p 6379:6379 \
  -p 9090:9090 \
  -v rustypotato-data:/data \
  -e RUSTYPOTATO_LOG_LEVEL=info \
  theprantadutta/rustypotato:latest \
  --aof-path /data/rustypotato.aof \
  --metrics-port 9090
```

#### Docker Compose

Create `docker-compose.yml`:

```yaml
version: '3.8'

services:
  rustypotato:
    image: theprantadutta/rustypotato:latest
    container_name: rustypotato
    ports:
      - "6379:6379"
      - "9090:9090"
    volumes:
      - rustypotato-data:/data
      - ./config/rustypotato.toml:/etc/rustypotato/rustypotato.toml:ro
    environment:
      - RUSTYPOTATO_LOG_LEVEL=info
      - RUSTYPOTATO_METRICS_ENABLED=true
    command: >
      --config /etc/rustypotato/rustypotato.toml
      --aof-path /data/rustypotato.aof
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "rustypotato-cli", "ping"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 40s

  # Optional: Prometheus for metrics collection
  prometheus:
    image: prom/prometheus:latest
    ports:
      - "9091:9090"
    volumes:
      - ./config/prometheus.yml:/etc/prometheus/prometheus.yml:ro
      - prometheus-data:/prometheus
    command:
      - '--config.file=/etc/prometheus/prometheus.yml'
      - '--storage.tsdb.path=/prometheus'
      - '--web.console.libraries=/etc/prometheus/console_libraries'
      - '--web.console.templates=/etc/prometheus/consoles'
    depends_on:
      - rustypotato

  # Optional: Grafana for visualization
  grafana:
    image: grafana/grafana:latest
    ports:
      - "3000:3000"
    volumes:
      - grafana-data:/var/lib/grafana
      - ./config/grafana/dashboards:/etc/grafana/provisioning/dashboards:ro
      - ./config/grafana/datasources:/etc/grafana/provisioning/datasources:ro
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=admin
    depends_on:
      - prometheus

volumes:
  rustypotato-data:
  prometheus-data:
  grafana-data:
```

Run with:

```bash
docker-compose up -d
```

### Production Docker Setup

#### Multi-stage Build for Custom Images

```dockerfile
# Dockerfile.production
FROM theprantadutta/rustypotato:latest

# Add custom configuration
COPY production.toml /etc/rustypotato/rustypotato.toml

# Add monitoring scripts
COPY scripts/health-check.sh /usr/local/bin/
COPY scripts/backup.sh /usr/local/bin/

# Set production defaults
ENV RUSTYPOTATO_LOG_LEVEL=warn
ENV RUSTYPOTATO_METRICS_ENABLED=true

# Use custom entrypoint
COPY scripts/entrypoint.sh /entrypoint.sh
ENTRYPOINT ["/entrypoint.sh"]
```

#### Docker Swarm Deployment

```yaml
# docker-stack.yml
version: '3.8'

services:
  rustypotato:
    image: theprantadutta/rustypotato:latest
    ports:
      - "6379:6379"
      - "9090:9090"
    volumes:
      - rustypotato-data:/data
    environment:
      - RUSTYPOTATO_LOG_LEVEL=info
    command: --aof-path /data/rustypotato.aof --metrics-port 9090
    deploy:
      replicas: 3
      placement:
        constraints:
          - node.role == worker
      resources:
        limits:
          memory: 2G
          cpus: '1.0'
        reservations:
          memory: 1G
          cpus: '0.5'
      restart_policy:
        condition: on-failure
        delay: 5s
        max_attempts: 3
      update_config:
        parallelism: 1
        delay: 10s
        failure_action: rollback

volumes:
  rustypotato-data:
    driver: local
```

Deploy with:

```bash
docker stack deploy -c docker-stack.yml rustypotato-stack
```

## Kubernetes Deployment

### Prerequisites

- Kubernetes cluster (1.20+)
- kubectl configured
- Persistent storage class available

### Basic Deployment

```bash
# Apply all manifests
kubectl apply -f k8s/

# Check deployment status
kubectl get pods -l app=rustypotato
kubectl get services rustypotato-service
```

### Production Kubernetes Setup

#### Namespace and RBAC

```yaml
# k8s/namespace.yaml
apiVersion: v1
kind: Namespace
metadata:
  name: rustypotato
  labels:
    name: rustypotato

---
apiVersion: v1
kind: ServiceAccount
metadata:
  name: rustypotato
  namespace: rustypotato

---
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  namespace: rustypotato
  name: rustypotato-role
rules:
- apiGroups: [""]
  resources: ["pods", "services", "endpoints"]
  verbs: ["get", "list", "watch"]

---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: rustypotato-rolebinding
  namespace: rustypotato
subjects:
- kind: ServiceAccount
  name: rustypotato
  namespace: rustypotato
roleRef:
  kind: Role
  name: rustypotato-role
  apiGroup: rbac.authorization.k8s.io
```

#### StatefulSet for Persistent Storage

```yaml
# k8s/statefulset.yaml
apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: rustypotato
  namespace: rustypotato
spec:
  serviceName: rustypotato-headless
  replicas: 3
  selector:
    matchLabels:
      app: rustypotato
  template:
    metadata:
      labels:
        app: rustypotato
    spec:
      serviceAccountName: rustypotato
      containers:
      - name: rustypotato
        image: rustypotato/rustypotato:latest
        ports:
        - containerPort: 6379
          name: redis
        - containerPort: 9090
          name: metrics
        env:
        - name: POD_NAME
          valueFrom:
            fieldRef:
              fieldPath: metadata.name
        - name: RUSTYPOTATO_BIND_ADDRESS
          value: "0.0.0.0"
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
      - name: config
        configMap:
          name: rustypotato-config
  volumeClaimTemplates:
  - metadata:
      name: data
    spec:
      accessModes: ["ReadWriteOnce"]
      storageClassName: fast-ssd
      resources:
        requests:
          storage: 20Gi
```

#### Ingress Configuration

```yaml
# k8s/ingress.yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: rustypotato-ingress
  namespace: rustypotato
  annotations:
    nginx.ingress.kubernetes.io/tcp-services-configmap: rustypotato/rustypotato-tcp
    nginx.ingress.kubernetes.io/stream-snippet: |
      upstream rustypotato {
          server rustypotato-service:6379;
      }
spec:
  rules:
  - host: rustypotato.example.com
    http:
      paths:
      - path: /metrics
        pathType: Prefix
        backend:
          service:
            name: rustypotato-service
            port:
              number: 9090

---
apiVersion: v1
kind: ConfigMap
metadata:
  name: rustypotato-tcp
  namespace: rustypotato
data:
  6379: "rustypotato/rustypotato-service:6379"
```

### Monitoring with Prometheus

```yaml
# k8s/servicemonitor.yaml
apiVersion: monitoring.coreos.com/v1
kind: ServiceMonitor
metadata:
  name: rustypotato
  namespace: rustypotato
  labels:
    app: rustypotato
spec:
  selector:
    matchLabels:
      app: rustypotato
  endpoints:
  - port: metrics
    interval: 30s
    path: /metrics
```

## Bare Metal Installation

### System Requirements

- **OS**: Linux (Ubuntu 20.04+, CentOS 8+, RHEL 8+)
- **CPU**: 2+ cores (4+ recommended for production)
- **Memory**: 4GB+ RAM (8GB+ recommended for production)
- **Storage**: SSD recommended for AOF files
- **Network**: Gigabit Ethernet recommended

### Installation Steps

#### Using Installation Script

```bash
# Download and run installation script
curl -fsSL https://install.rustypotato.dev | bash

# Or download and inspect first
curl -fsSL https://install.rustypotato.dev -o install.sh
chmod +x install.sh
./install.sh
```

#### Manual Installation

```bash
# Create user and directories
sudo useradd -r -s /bin/false rustypotato
sudo mkdir -p /etc/rustypotato /var/lib/rustypotato /var/log/rustypotato
sudo chown rustypotato:rustypotato /var/lib/rustypotato /var/log/rustypotato

# Download and install binaries
curl -L https://github.com/theprantadutta/rustypotato/releases/latest/download/rustypotato-linux-x64.tar.gz | tar xz
sudo cp rustypotato-linux-x64/rustypotato-* /usr/local/bin/
sudo chmod +x /usr/local/bin/rustypotato-*

# Create configuration
sudo tee /etc/rustypotato/rustypotato.toml > /dev/null <<EOF
[server]
port = 6379
bind_address = "127.0.0.1"
max_connections = 10000

[storage]
aof_enabled = true
aof_path = "/var/lib/rustypotato/rustypotato.aof"
aof_fsync = "everysec"

[logging]
level = "info"
file = "/var/log/rustypotato/rustypotato.log"

[metrics]
enabled = true
port = 9090
EOF

# Create systemd service
sudo tee /etc/systemd/system/rustypotato.service > /dev/null <<EOF
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

[Install]
WantedBy=multi-user.target
EOF

# Enable and start service
sudo systemctl daemon-reload
sudo systemctl enable rustypotato
sudo systemctl start rustypotato
```

### System Tuning

#### Kernel Parameters

```bash
# /etc/sysctl.d/99-rustypotato.conf
net.core.somaxconn = 65535
net.ipv4.tcp_max_syn_backlog = 65535
net.core.netdev_max_backlog = 5000
net.ipv4.tcp_fin_timeout = 30
net.ipv4.tcp_keepalive_time = 120
net.ipv4.tcp_keepalive_probes = 3
net.ipv4.tcp_keepalive_intvl = 15
vm.overcommit_memory = 1
vm.swappiness = 1

# Apply changes
sudo sysctl -p /etc/sysctl.d/99-rustypotato.conf
```

#### File Limits

```bash
# /etc/security/limits.d/99-rustypotato.conf
rustypotato soft nofile 65536
rustypotato hard nofile 65536
rustypotato soft nproc 32768
rustypotato hard nproc 32768
```

## Cloud Deployments

### AWS

#### ECS Fargate

```json
{
  "family": "rustypotato",
  "networkMode": "awsvpc",
  "requiresCompatibilities": ["FARGATE"],
  "cpu": "1024",
  "memory": "2048",
  "executionRoleArn": "arn:aws:iam::ACCOUNT:role/ecsTaskExecutionRole",
  "taskRoleArn": "arn:aws:iam::ACCOUNT:role/rustypotato-task-role",
  "containerDefinitions": [
    {
      "name": "rustypotato",
      "image": "rustypotato/rustypotato:latest",
      "portMappings": [
        {
          "containerPort": 6379,
          "protocol": "tcp"
        },
        {
          "containerPort": 9090,
          "protocol": "tcp"
        }
      ],
      "environment": [
        {
          "name": "RUSTYPOTATO_LOG_LEVEL",
          "value": "info"
        }
      ],
      "mountPoints": [
        {
          "sourceVolume": "rustypotato-data",
          "containerPath": "/data"
        }
      ],
      "logConfiguration": {
        "logDriver": "awslogs",
        "options": {
          "awslogs-group": "/ecs/rustypotato",
          "awslogs-region": "us-west-2",
          "awslogs-stream-prefix": "ecs"
        }
      },
      "healthCheck": {
        "command": ["CMD-SHELL", "rustypotato-cli ping || exit 1"],
        "interval": 30,
        "timeout": 5,
        "retries": 3,
        "startPeriod": 60
      }
    }
  ],
  "volumes": [
    {
      "name": "rustypotato-data",
      "efsVolumeConfiguration": {
        "fileSystemId": "fs-12345678",
        "transitEncryption": "ENABLED"
      }
    }
  ]
}
```

#### EKS

```bash
# Create EKS cluster
eksctl create cluster --name rustypotato-cluster --region us-west-2

# Deploy RustyPotato
kubectl apply -f k8s/

# Create load balancer
kubectl expose deployment rustypotato --type=LoadBalancer --port=6379
```

### Google Cloud Platform

#### GKE

```bash
# Create GKE cluster
gcloud container clusters create rustypotato-cluster \
  --zone us-central1-a \
  --num-nodes 3 \
  --machine-type n1-standard-2

# Deploy RustyPotato
kubectl apply -f k8s/
```

#### Cloud Run

```yaml
# cloudrun.yaml
apiVersion: serving.knative.dev/v1
kind: Service
metadata:
  name: rustypotato
  annotations:
    run.googleapis.com/ingress: all
spec:
  template:
    metadata:
      annotations:
        autoscaling.knative.dev/maxScale: "10"
        run.googleapis.com/cpu-throttling: "false"
        run.googleapis.com/memory: "2Gi"
        run.googleapis.com/cpu: "1000m"
    spec:
      containers:
      - image: rustypotato/rustypotato:latest
        ports:
        - containerPort: 6379
        env:
        - name: RUSTYPOTATO_BIND_ADDRESS
          value: "0.0.0.0"
        resources:
          limits:
            memory: "2Gi"
            cpu: "1000m"
```

### Azure

#### AKS

```bash
# Create AKS cluster
az aks create \
  --resource-group rustypotato-rg \
  --name rustypotato-cluster \
  --node-count 3 \
  --node-vm-size Standard_D2s_v3 \
  --enable-addons monitoring

# Deploy RustyPotato
kubectl apply -f k8s/
```

## Configuration

### Configuration File Structure

```toml
# /etc/rustypotato/rustypotato.toml

[server]
port = 6379
bind_address = "0.0.0.0"
max_connections = 10000
tcp_keepalive = true
tcp_nodelay = true

[storage]
# Persistence
aof_enabled = true
aof_path = "/data/rustypotato.aof"
aof_fsync = "everysec"  # always, everysec, no

# Memory management
max_memory = "2GB"
eviction_policy = "lru"  # lru, lfu, random, ttl

[logging]
level = "info"  # trace, debug, info, warn, error
format = "json"  # json, pretty
file = "/var/log/rustypotato/rustypotato.log"
rotation = "daily"  # daily, hourly, size

[metrics]
enabled = true
port = 9090
path = "/metrics"

[security]
# Authentication (when implemented)
auth_enabled = false
password = ""

# TLS (when implemented)
tls_enabled = false
cert_file = ""
key_file = ""
```

### Environment Variables

All configuration options can be overridden with environment variables:

```bash
export RUSTYPOTATO_PORT=6380
export RUSTYPOTATO_BIND_ADDRESS="0.0.0.0"
export RUSTYPOTATO_AOF_PATH="/data/rustypotato.aof"
export RUSTYPOTATO_LOG_LEVEL="debug"
export RUSTYPOTATO_MAX_MEMORY="4GB"
```

## Monitoring and Observability

### Metrics

RustyPotato exposes Prometheus-compatible metrics on `/metrics` endpoint:

```bash
# Check metrics endpoint
curl http://localhost:9090/metrics
```

Key metrics include:
- `rustypotato_commands_total` - Total commands executed
- `rustypotato_command_duration_seconds` - Command execution time
- `rustypotato_memory_usage_bytes` - Memory usage
- `rustypotato_connections_active` - Active connections
- `rustypotato_aof_writes_total` - AOF write operations

### Logging

Structured logging with configurable levels and formats:

```bash
# View logs (systemd)
journalctl -u rustypotato -f

# View logs (Docker)
docker logs -f rustypotato

# View logs (Kubernetes)
kubectl logs -f deployment/rustypotato
```

### Health Checks

```bash
# Basic health check
rustypotato-cli ping

# Detailed server info
rustypotato-cli info

# Check metrics endpoint
curl http://localhost:9090/health
```

## Security

### Network Security

```bash
# Firewall rules (iptables)
sudo iptables -A INPUT -p tcp --dport 6379 -s 10.0.0.0/8 -j ACCEPT
sudo iptables -A INPUT -p tcp --dport 6379 -j DROP

# Firewall rules (ufw)
sudo ufw allow from 10.0.0.0/8 to any port 6379
sudo ufw deny 6379
```

### TLS Configuration (Future Feature)

```toml
[security]
tls_enabled = true
cert_file = "/etc/rustypotato/tls/server.crt"
key_file = "/etc/rustypotato/tls/server.key"
ca_file = "/etc/rustypotato/tls/ca.crt"
```

### Authentication (Future Feature)

```toml
[security]
auth_enabled = true
password = "your-secure-password"
```

## Performance Tuning

### Memory Optimization

```toml
[storage]
max_memory = "8GB"
eviction_policy = "lru"
```

### Network Optimization

```toml
[server]
tcp_keepalive = true
tcp_nodelay = true
max_connections = 50000
```

### Persistence Optimization

```toml
[storage]
aof_fsync = "everysec"  # Balance between durability and performance
```

### System-level Tuning

```bash
# Increase file descriptor limits
echo "* soft nofile 65536" >> /etc/security/limits.conf
echo "* hard nofile 65536" >> /etc/security/limits.conf

# Optimize TCP settings
echo "net.core.somaxconn = 65535" >> /etc/sysctl.conf
echo "net.ipv4.tcp_max_syn_backlog = 65535" >> /etc/sysctl.conf
sysctl -p
```

## Backup and Recovery

### AOF Backup

```bash
# Create backup
cp /var/lib/rustypotato/rustypotato.aof /backup/rustypotato-$(date +%Y%m%d).aof

# Automated backup script
#!/bin/bash
BACKUP_DIR="/backup"
AOF_FILE="/var/lib/rustypotato/rustypotato.aof"
DATE=$(date +%Y%m%d-%H%M%S)

# Create backup
cp "$AOF_FILE" "$BACKUP_DIR/rustypotato-$DATE.aof"

# Compress old backups
find "$BACKUP_DIR" -name "*.aof" -mtime +1 -exec gzip {} \;

# Remove old backups (keep 30 days)
find "$BACKUP_DIR" -name "*.aof.gz" -mtime +30 -delete
```

### Recovery

```bash
# Stop RustyPotato
sudo systemctl stop rustypotato

# Restore from backup
cp /backup/rustypotato-20241201.aof /var/lib/rustypotato/rustypotato.aof
chown rustypotato:rustypotato /var/lib/rustypotato/rustypotato.aof

# Start RustyPotato
sudo systemctl start rustypotato
```

## Troubleshooting

### Common Issues

#### Connection Refused

```bash
# Check if service is running
sudo systemctl status rustypotato

# Check port binding
sudo netstat -tlnp | grep 6379

# Check firewall
sudo iptables -L | grep 6379
```

#### High Memory Usage

```bash
# Check memory usage
rustypotato-cli info memory

# Monitor with top
top -p $(pgrep rustypotato-server)

# Check for memory leaks
valgrind --tool=memcheck --leak-check=full rustypotato-server
```

#### Performance Issues

```bash
# Check system resources
htop

# Monitor network connections
ss -tuln | grep 6379

# Check disk I/O
iostat -x 1

# Profile with perf
perf record -g rustypotato-server
perf report
```

### Log Analysis

```bash
# Search for errors
journalctl -u rustypotato | grep ERROR

# Monitor real-time logs
journalctl -u rustypotato -f

# Check specific time range
journalctl -u rustypotato --since "2024-01-01 00:00:00" --until "2024-01-01 23:59:59"
```

### Debug Mode

```bash
# Run with debug logging
RUSTYPOTATO_LOG_LEVEL=debug rustypotato-server

# Enable trace logging
RUSTYPOTATO_LOG_LEVEL=trace rustypotato-server
```

For additional support, please check:
- [GitHub Issues](https://github.com/theprantadutta/rustypotato/issues)
- [Documentation Wiki](https://github.com/theprantadutta/rustypotato/wiki)
- [Community Discussions](https://github.com/theprantadutta/rustypotato/discussions)