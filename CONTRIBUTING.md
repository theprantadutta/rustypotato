# Contributing to RustyPotato

Thank you for your interest in contributing to RustyPotato! This document provides guidelines and information for contributors.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [Contributing Process](#contributing-process)
- [Coding Standards](#coding-standards)
- [Testing Guidelines](#testing-guidelines)
- [Documentation](#documentation)
- [Performance Considerations](#performance-considerations)
- [Security](#security)

## Code of Conduct

This project follows the [Rust Code of Conduct](https://www.rust-lang.org/policies/code-of-conduct). Please read and follow it in all interactions.

## Getting Started

### Prerequisites

- Rust 1.70+ (install via [rustup](https://rustup.rs/))
- Git
- Docker (optional, for integration tests)

### Development Tools

Install recommended development tools:

```bash
# Code formatting and linting
rustup component add rustfmt clippy

# Additional tools
cargo install cargo-watch cargo-tarpaulin cargo-audit cargo-deny
```

## Development Setup

1. **Fork and Clone**
   ```bash
   git clone https://github.com/YOUR_USERNAME/rustypotato.git
   cd rustypotato
   ```

2. **Build and Test**
   ```bash
   cargo build
   cargo test
   ```

3. **Run Development Server**
   ```bash
   cargo run --bin rustypotato-server
   ```

4. **Auto-reload During Development**
   ```bash
   cargo watch -x run
   ```

## Contributing Process

### 1. Issue First

- Check existing issues before creating new ones
- For bugs, provide minimal reproduction steps
- For features, discuss the design first
- Use issue templates when available

### 2. Branch Strategy

- Create feature branches from `main`
- Use descriptive branch names: `feature/add-hash-commands`, `fix/memory-leak`
- Keep branches focused and short-lived

### 3. Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add HSET command for hash operations
fix: resolve memory leak in connection handling
docs: update installation instructions
test: add integration tests for persistence
perf: optimize memory allocation in hot paths
```

### 4. Pull Request Process

1. **Before Submitting**
   - Ensure all tests pass: `cargo test`
   - Format code: `cargo fmt`
   - Lint code: `cargo clippy -- -D warnings`
   - Update documentation if needed

2. **PR Description**
   - Clearly describe the changes
   - Reference related issues
   - Include testing instructions
   - Add screenshots for UI changes

3. **Review Process**
   - Address reviewer feedback promptly
   - Keep discussions constructive
   - Update PR based on feedback

## Coding Standards

### Rust Style

- Follow standard Rust naming conventions
- Use `rustfmt` with default settings
- Prefer explicit types when it improves readability
- Use meaningful variable and function names

### Error Handling

```rust
// Good: Use Result for fallible operations
pub fn get_value(&self, key: &str) -> Result<Option<StoredValue>, RustyPotatoError> {
    self.store.get(key)
        .map_err(|e| RustyPotatoError::StorageError(e.to_string()))
}

// Bad: Never use unwrap() in production code
pub fn get_value(&self, key: &str) -> Option<StoredValue> {
    self.store.get(key).unwrap() // Don't do this!
}
```

### Memory Management

- Prefer owned types (`String`, `Vec`) in struct fields
- Use `Arc<T>` for shared ownership in concurrent contexts
- Minimize `clone()` calls in hot paths
- Use `Cow<str>` when you might need either borrowed or owned strings

### Concurrency

- Use `tokio` for all async operations
- Prefer `DashMap` over `Arc<Mutex<HashMap>>`
- Never use `std::sync::Mutex` with async code
- Use `tokio::sync::Mutex` for async-aware locking

## Testing Guidelines

### Test Categories

1. **Unit Tests** (`#[cfg(test)]` modules)
   - Test individual functions and methods
   - Mock external dependencies
   - Fast execution (< 1ms per test)

2. **Integration Tests** (`tests/` directory)
   - Test component interactions
   - Use real implementations
   - Test complete workflows

3. **Performance Tests** (`benches/` directory)
   - Benchmark critical paths
   - Measure latency and throughput
   - Prevent performance regressions

### Test Structure

Use the Arrange-Act-Assert pattern:

```rust
#[tokio::test]
async fn test_set_overwrites_existing_key() {
    // Arrange
    let store = MemoryStore::new();
    store.set("key1", "original_value").await.unwrap();
    
    // Act
    let result = store.set("key1", "new_value").await;
    
    // Assert
    assert!(result.is_ok());
    assert_eq!(store.get("key1").await.unwrap(), Some("new_value"));
}
```

### Test Naming

- Use descriptive names: `test_get_returns_none_for_missing_key`
- Include expected behavior: `test_expire_removes_key_after_timeout`
- Use `should` for behavior: `test_concurrent_access_should_maintain_consistency`

## Documentation

### Code Documentation

- Document all public APIs with `///` comments
- Include examples in documentation
- Document panics, errors, and safety requirements
- Use `#[doc(hidden)]` for internal APIs

```rust
/// Sets a key-value pair with optional expiration.
/// 
/// # Arguments
/// 
/// * `key` - The key to set
/// * `value` - The value to store
/// * `ttl` - Optional time-to-live in seconds
/// 
/// # Examples
/// 
/// ```
/// # use rustypotato::MemoryStore;
/// # tokio_test::block_on(async {
/// let store = MemoryStore::new();
/// store.set_with_ttl("key", "value", Some(60)).await?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// # });
/// ```
/// 
/// # Errors
/// 
/// Returns `RustyPotatoError::StorageError` if the operation fails.
pub async fn set_with_ttl(&self, key: &str, value: &str, ttl: Option<u64>) -> Result<()> {
    // Implementation
}
```

### README and Guides

- Keep README focused on getting started
- Provide architecture overview in separate docs
- Include performance characteristics
- Document configuration options thoroughly

## Performance Considerations

### Hot Path Optimization

- Minimize allocations in frequently called functions
- Use `&str` instead of `String` for temporary values
- Prefer `Vec::with_capacity()` when size is known
- Profile before optimizing

### Memory Efficiency

- Use appropriate collection types
- Consider memory layout for frequently accessed structs
- Use `Box<[T]>` instead of `Vec<T>` for fixed-size collections
- Monitor memory usage in tests

### Async Best Practices

- Use `spawn` for CPU-intensive tasks
- Prefer `select!` over manual future polling
- Use bounded channels to prevent memory leaks
- Always handle channel closure gracefully

## Security

### Input Validation

- Validate all external input at boundaries
- Use type system to enforce constraints
- Sanitize data before logging
- Implement rate limiting for network operations

### Memory Safety

- Never use `unsafe` without thorough justification
- Document all `unsafe` blocks with safety invariants
- Prefer safe abstractions over raw pointers
- Use `#[forbid(unsafe_code)]` where possible

## Release Process

### Version Numbering

We follow [Semantic Versioning](https://semver.org/):

- `MAJOR.MINOR.PATCH`
- Breaking changes increment MAJOR
- New features increment MINOR
- Bug fixes increment PATCH

### Release Checklist

1. Update version in `Cargo.toml`
2. Update `CHANGELOG.md`
3. Run full test suite
4. Update documentation
5. Create release PR
6. Tag release after merge
7. GitHub Actions handles the rest

## Getting Help

- **Questions**: Open a discussion on GitHub
- **Bugs**: Create an issue with reproduction steps
- **Features**: Discuss in issues before implementing
- **Security**: Email security@rustypotato.dev (if applicable)

## Recognition

Contributors are recognized in:

- `CONTRIBUTORS.md` file
- Release notes
- GitHub contributors page
- Special recognition for significant contributions

Thank you for contributing to RustyPotato! 🥔