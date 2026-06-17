//! Fuzz target: parse arbitrary input as a `ComposeSpec`. Catches
//! parser DoS, panics on malformed YAML, integer overflow in field
//! parsing, etc. Run via `cargo +nightly fuzz run compose_yaml_parse`.

#![no_main]

use libfuzzer_sys::fuzz_target;
use perry_container_compose::types::ComposeSpec;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Deliberately ignore the result — we're checking that parsing
        // *terminates* and *doesn't panic*. Errors are fine.
        let _ = ComposeSpec::parse_str(s);
    }
});
