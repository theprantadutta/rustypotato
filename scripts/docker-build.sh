#!/bin/bash
set -e

# RustyPotato Docker Build Script

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
REGISTRY="theprantadutta"
IMAGE_NAME="rustypotato"
PLATFORMS="linux/amd64,linux/arm64,linux/arm/v7"

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

log_step() {
    echo -e "${BLUE}[STEP]${NC} $1"
}

# Get version from Cargo.toml or use provided argument
get_version() {
    if [ -n "$1" ]; then
        VERSION="$1"
    else
        VERSION=$(grep '^version = ' Cargo.toml | sed 's/version = "\(.*\)"/\1/')
    fi
    
    if [ -z "$VERSION" ]; then
        log_error "Could not determine version"
        exit 1
    fi
    
    log_info "Building version: $VERSION"
}

# Check prerequisites
check_prerequisites() {
    log_step "Checking prerequisites..."
    
    # Check if Docker is installed and running
    if ! command -v docker &> /dev/null; then
        log_error "Docker is not installed"
        exit 1
    fi
    
    if ! docker info &> /dev/null; then
        log_error "Docker daemon is not running"
        exit 1
    fi
    
    # Check if buildx is available
    if ! docker buildx version &> /dev/null; then
        log_error "Docker buildx is not available"
        exit 1
    fi
    
    log_info "Prerequisites check passed"
}

# Setup buildx builder
setup_buildx() {
    log_step "Setting up Docker buildx..."
    
    # Create builder if it doesn't exist
    if ! docker buildx inspect rustypotato-builder &> /dev/null; then
        log_info "Creating buildx builder..."
        docker buildx create --name rustypotato-builder --use
    else
        log_info "Using existing buildx builder..."
        docker buildx use rustypotato-builder
    fi
    
    # Bootstrap the builder
    docker buildx inspect --bootstrap
}

# Build multi-architecture images
build_images() {
    log_step "Building Docker images..."
    
    local tags=""
    
    # Add version tag
    tags="$tags -t $REGISTRY/$IMAGE_NAME:$VERSION"
    
    # Add latest tag if not a pre-release
    if [[ ! "$VERSION" =~ (alpha|beta|rc) ]]; then
        tags="$tags -t $REGISTRY/$IMAGE_NAME:latest"
    fi
    
    # Add major.minor tag
    local major_minor=$(echo "$VERSION" | sed 's/\([0-9]*\.[0-9]*\).*/\1/')
    tags="$tags -t $REGISTRY/$IMAGE_NAME:$major_minor"
    
    log_info "Building with tags: $tags"
    
    # Build and push
    docker buildx build \
        --platform "$PLATFORMS" \
        --push \
        $tags \
        --build-arg VERSION="$VERSION" \
        --build-arg BUILD_DATE="$(date -u +'%Y-%m-%dT%H:%M:%SZ')" \
        --build-arg VCS_REF="$(git rev-parse HEAD)" \
        .
    
    log_info "Images built and pushed successfully"
}

# Build local development image
build_local() {
    log_step "Building local development image..."
    
    docker build \
        -t "$REGISTRY/$IMAGE_NAME:dev" \
        --build-arg VERSION="$VERSION-dev" \
        --build-arg BUILD_DATE="$(date -u +'%Y-%m-%dT%H:%M:%SZ')" \
        --build-arg VCS_REF="$(git rev-parse HEAD)" \
        .
    
    log_info "Local development image built: $REGISTRY/$IMAGE_NAME:dev"
}

# Test the built image
test_image() {
    log_step "Testing Docker image..."
    
    local test_image="$REGISTRY/$IMAGE_NAME:dev"
    
    # Test server startup
    log_info "Testing server startup..."
    local container_id=$(docker run -d --rm -p 16379:6379 "$test_image")
    
    # Wait for server to start
    sleep 5
    
    # Test connection
    if docker exec "$container_id" rustypotato-cli ping; then
        log_info "Server test passed"
    else
        log_error "Server test failed"
        docker logs "$container_id"
        docker stop "$container_id"
        exit 1
    fi
    
    # Test CLI
    log_info "Testing CLI commands..."
    docker exec "$container_id" rustypotato-cli set test_key "test_value"
    local result=$(docker exec "$container_id" rustypotato-cli get test_key)
    
    if [ "$result" = "test_value" ]; then
        log_info "CLI test passed"
    else
        log_error "CLI test failed: expected 'test_value', got '$result'"
        docker stop "$container_id"
        exit 1
    fi
    
    # Cleanup
    docker stop "$container_id"
    log_info "Image tests passed"
}

# Generate docker-compose.yml for testing
generate_compose() {
    log_step "Generating docker-compose.yml..."
    
    cat > docker-compose.yml <<EOF
version: '3.8'

services:
  rustypotato:
    image: $REGISTRY/$IMAGE_NAME:$VERSION
    container_name: rustypotato
    ports:
      - "6379:6379"
      - "9090:9090"
    volumes:
      - rustypotato-data:/data
      - ./docker/rustypotato.toml:/etc/rustypotato/rustypotato.toml
    environment:
      - RUSTYPOTATO_LOG_LEVEL=info
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "rustypotato-cli", "ping"]
      interval: 30s
      timeout: 10s
      retries: 3

volumes:
  rustypotato-data:
EOF
    
    log_info "docker-compose.yml generated"
}

# Show usage
usage() {
    echo "Usage: $0 [OPTIONS] [VERSION]"
    echo ""
    echo "Options:"
    echo "  --local     Build local development image only"
    echo "  --test      Test the built image"
    echo "  --compose   Generate docker-compose.yml"
    echo "  --help      Show this help message"
    echo ""
    echo "Examples:"
    echo "  $0                    # Build and push release images"
    echo "  $0 --local           # Build local development image"
    echo "  $0 --test            # Build and test local image"
    echo "  $0 1.0.0             # Build specific version"
}

# Main function
main() {
    local local_only=false
    local test_image_flag=false
    local generate_compose_flag=false
    local version_arg=""
    
    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            --local)
                local_only=true
                shift
                ;;
            --test)
                test_image_flag=true
                shift
                ;;
            --compose)
                generate_compose_flag=true
                shift
                ;;
            --help)
                usage
                exit 0
                ;;
            -*)
                log_error "Unknown option: $1"
                usage
                exit 1
                ;;
            *)
                version_arg="$1"
                shift
                ;;
        esac
    done
    
    get_version "$version_arg"
    check_prerequisites
    
    if [ "$local_only" = true ]; then
        build_local
    else
        setup_buildx
        build_images
        build_local
    fi
    
    if [ "$test_image_flag" = true ]; then
        test_image
    fi
    
    if [ "$generate_compose_flag" = true ]; then
        generate_compose
    fi
    
    log_info "Docker build completed successfully!"
    
    if [ "$local_only" = false ]; then
        log_info "Images pushed to registry:"
        echo "  $REGISTRY/$IMAGE_NAME:$VERSION"
        echo "  $REGISTRY/$IMAGE_NAME:latest"
    fi
    
    log_info "Local development image: $REGISTRY/$IMAGE_NAME:dev"
}

# Run main function
main "$@"