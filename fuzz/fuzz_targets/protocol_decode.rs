//! Fuzz target for RESP protocol decoding
//!
//! This fuzzer tests the RESP protocol parser with arbitrary byte sequences
//! to find parsing bugs, panics, or undefined behavior.
//!
//! Run with: cargo +nightly fuzz run protocol_decode
//!
//! Note: cargo-fuzz requires Linux/macOS. On Windows, run in WSL or CI.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rustypotato::network::protocol::RespCodec;

fuzz_target!(|data: &[u8]| {
    // Create a fresh codec for each input
    let mut codec = RespCodec::new();

    // Try to decode the arbitrary bytes
    // The goal is to ensure this never panics, even on malformed input
    let _ = codec.decode(data);

    // Also test incremental decoding by feeding bytes in chunks
    if data.len() > 2 {
        let mut codec2 = RespCodec::new();
        let mid = data.len() / 2;

        // Feed first half
        let _ = codec2.decode(&data[..mid]);

        // Feed second half
        let _ = codec2.decode(&data[mid..]);
    }
});
