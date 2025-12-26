# RustyPotato Fuzzing

This directory contains fuzzing targets for finding bugs in the RESP protocol implementation and command execution.

## Requirements

- **Linux or macOS** (cargo-fuzz uses libFuzzer which doesn't work on Windows)
- Rust nightly toolchain
- cargo-fuzz: `cargo install cargo-fuzz`

## Fuzz Targets

| Target | Purpose |
|--------|---------|
| `protocol_decode` | Tests RESP parser with arbitrary bytes to find parsing bugs/panics |
| `command_execution` | Tests command validation and execution with arbitrary inputs |
| `resp_roundtrip` | Tests that ResponseValue encoding produces valid RESP |

## Running Fuzz Tests

```bash
# Install cargo-fuzz (one-time)
cargo install cargo-fuzz

# Switch to nightly
rustup default nightly

# Run a specific fuzz target
cargo +nightly fuzz run protocol_decode

# Run with a time limit (e.g., 60 seconds)
cargo +nightly fuzz run protocol_decode -- -max_total_time=60

# Run with more jobs for faster coverage
cargo +nightly fuzz run protocol_decode -- -jobs=4

# List available targets
cargo +nightly fuzz list
```

## Running on Windows

The recommended approach for Windows users:

1. **WSL2**: Install Windows Subsystem for Linux and run fuzzing inside WSL
2. **CI/CD**: Run fuzzing in GitHub Actions or other Linux-based CI
3. **Docker**: Use a Linux container

## Interpreting Results

Crashes are saved in `fuzz/artifacts/<target_name>/`. Each crash file contains the minimal input that triggers the bug.

To reproduce a crash:
```bash
cargo +nightly fuzz run protocol_decode fuzz/artifacts/protocol_decode/crash-xxx
```

## CI Integration

Add this to your GitHub Actions workflow:

```yaml
fuzz:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@nightly
    - run: cargo install cargo-fuzz
    - run: cargo +nightly fuzz run protocol_decode -- -max_total_time=300
    - run: cargo +nightly fuzz run command_execution -- -max_total_time=300
    - run: cargo +nightly fuzz run resp_roundtrip -- -max_total_time=300
```
