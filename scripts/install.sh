#!/bin/bash
set -e

# RustyPotato Installation Script
# This script downloads and installs the latest RustyPotato release

REPO="theprantadutta/rustypotato"
INSTALL_DIR="/usr/local/bin"
CONFIG_DIR="/etc/rustypotato"
DATA_DIR="/var/lib/rustypotato"
LOG_DIR="/var/log/rustypotato"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Logging functions
log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Detect OS and architecture
detect_platform() {
    local os=$(uname -s | tr '[:upper:]' '[:lower:]')
    local arch=$(uname -m)
    
    case $os in
        linux)
            OS="linux"
            ;;
        darwin)
            OS="macos"
            ;;
        *)
            log_error "Unsupported operating system: $os"
            exit 1
            ;;
    esac
    
    case $arch in
        x86_64|amd64)
            ARCH="x64"
            ;;
        aarch64|arm64)
            ARCH="arm64"
            ;;
        *)
            log_error "Unsupported architecture: $arch"
            exit 1
            ;;
    esac
    
    PLATFORM="${OS}-${ARCH}"
    log_info "Detected platform: $PLATFORM"
}

# Get latest release version
get_latest_version() {
    log_info "Fetching latest release information..."
    LATEST_VERSION=$(curl -s "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
    
    if [ -z "$LATEST_VERSION" ]; then
        log_error "Failed to fetch latest version"
        exit 1
    fi
    
    log_info "Latest version: $LATEST_VERSION"
}

# Download and install binaries
install_binaries() {
    local download_url="https://github.com/$REPO/releases/download/$LATEST_VERSION/rustypotato-$PLATFORM.tar.gz"
    local temp_dir=$(mktemp -d)
    
    log_info "Downloading RustyPotato $LATEST_VERSION for $PLATFORM..."
    
    if ! curl -L "$download_url" -o "$temp_dir/rustypotato.tar.gz"; then
        log_error "Failed to download RustyPotato"
        exit 1
    fi
    
    log_info "Extracting binaries..."
    tar -xzf "$temp_dir/rustypotato.tar.gz" -C "$temp_dir"
    
    # Install binaries
    log_info "Installing binaries to $INSTALL_DIR..."
    sudo cp "$temp_dir/rustypotato-$PLATFORM/rustypotato-server" "$INSTALL_DIR/"
    sudo cp "$temp_dir/rustypotato-$PLATFORM/rustypotato-cli" "$INSTALL_DIR/"
    sudo chmod +x "$INSTALL_DIR/rustypotato-server" "$INSTALL_DIR/rustypotato-cli"
    
    # Cleanup
    rm -rf "$temp_dir"
    
    log_info "Binaries installed successfully"
}

# Create directories and configuration
setup_directories() {
    log_info "Setting up directories..."
    
    sudo mkdir -p "$CONFIG_DIR" "$DATA_DIR" "$LOG_DIR"
    
    # Create rustypotato user if it doesn't exist
    if ! id "rustypotato" &>/dev/null; then
        log_info "Creating rustypotato user..."
        sudo useradd -r -s /bin/false -d "$DATA_DIR" rustypotato
    fi
    
    # Set permissions
    sudo chown -R rustypotato:rustypotato "$DATA_DIR" "$LOG_DIR"
    sudo chmod 755 "$CONFIG_DIR"
    sudo chmod 750 "$DATA_DIR" "$LOG_DIR"
}

# Create default configuration
create_config() {
    local config_file="$CONFIG_DIR/rustypotato.toml"
    
    if [ ! -f "$config_file" ]; then
        log_info "Creating default configuration..."
        
        sudo tee "$config_file" > /dev/null <<EOF
# RustyPotato Configuration

[server]
port = 6379
bind_address = "127.0.0.1"
max_connections = 10000

[storage]
aof_enabled = true
aof_path = "$DATA_DIR/rustypotato.aof"
aof_fsync = "everysec"
max_memory = "1GB"

[logging]
level = "info"
format = "pretty"
file = "$LOG_DIR/rustypotato.log"

[metrics]
enabled = true
port = 9090

[security]
auth_enabled = false
EOF
        
        sudo chmod 644 "$config_file"
        log_info "Configuration created at $config_file"
    else
        log_warn "Configuration file already exists at $config_file"
    fi
}

# Create systemd service
create_systemd_service() {
    local service_file="/etc/systemd/system/rustypotato.service"
    
    if [ ! -f "$service_file" ]; then
        log_info "Creating systemd service..."
        
        sudo tee "$service_file" > /dev/null <<EOF
[Unit]
Description=RustyPotato Key-Value Store
Documentation=https://github.com/theprantadutta/rustypotato
After=network.target

[Service]
Type=simple
User=rustypotato
Group=rustypotato
ExecStart=$INSTALL_DIR/rustypotato-server --config $CONFIG_DIR/rustypotato.toml
ExecReload=/bin/kill -HUP \$MAINPID
Restart=always
RestartSec=5
LimitNOFILE=65536
WorkingDirectory=$DATA_DIR

# Security settings
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=$DATA_DIR $LOG_DIR

[Install]
WantedBy=multi-user.target
EOF
        
        sudo systemctl daemon-reload
        log_info "Systemd service created"
    else
        log_warn "Systemd service already exists"
    fi
}

# Main installation function
main() {
    log_info "Starting RustyPotato installation..."
    
    # Check if running as root for system installation
    if [ "$EUID" -eq 0 ]; then
        log_error "Please run this script as a regular user (it will use sudo when needed)"
        exit 1
    fi
    
    # Check dependencies
    for cmd in curl tar sudo; do
        if ! command -v $cmd &> /dev/null; then
            log_error "$cmd is required but not installed"
            exit 1
        fi
    done
    
    detect_platform
    get_latest_version
    install_binaries
    setup_directories
    create_config
    
    # Create systemd service on Linux
    if [ "$OS" = "linux" ] && command -v systemctl &> /dev/null; then
        create_systemd_service
        
        log_info "To start RustyPotato:"
        echo "  sudo systemctl enable rustypotato"
        echo "  sudo systemctl start rustypotato"
        echo ""
        log_info "To check status:"
        echo "  sudo systemctl status rustypotato"
    fi
    
    log_info "Installation completed successfully!"
    log_info "Configuration file: $CONFIG_DIR/rustypotato.toml"
    log_info "Data directory: $DATA_DIR"
    log_info "Log directory: $LOG_DIR"
    echo ""
    log_info "To test the installation:"
    echo "  rustypotato-server --help"
    echo "  rustypotato-cli --help"
}

# Run main function
main "$@"