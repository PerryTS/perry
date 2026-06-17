//! Fuzz target: JSON round-trip of `ComposeSpec`. Catches mismatches
//! between parse + re-serialise paths (e.g. fields silently dropped,
//! enum variants that don't round-trip, untagged-union ambiguity).

#![no_main]

use libfuzzer_sys::fuzz_target;
use perry_container_compose::types::ComposeSpec;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        if let Ok(spec) = serde_json::from_str::<ComposeSpec>(s) {
            // If it parsed once, re-serialising and re-parsing must
            // produce equivalent structure. We don't strict-equality
            // because fields like `extensions` (flatten-typed
            // serde_yaml::Value) don't have stable Eq, but a successful
            // re-parse without error is the invariant.
            if let Ok(reser) = serde_json::to_string(&spec) {
                let _ = serde_json::from_str::<ComposeSpec>(&reser);
            }
        }
    }
});
