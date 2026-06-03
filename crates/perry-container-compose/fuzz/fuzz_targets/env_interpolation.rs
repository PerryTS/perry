//! Fuzz target: env-var interpolation. Catches `${...}` parser DoS
//! (e.g. unbalanced braces, deeply nested defaults, recursive refs).

#![no_main]

use libfuzzer_sys::fuzz_target;
use perry_container_compose::yaml::interpolate;
use std::collections::HashMap;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let mut env = HashMap::new();
        env.insert("FOO".to_string(), "foo-value".to_string());
        env.insert("BAR".to_string(), "bar-value".to_string());
        // Must terminate without panic, regardless of input shape.
        let _ = interpolate(s, &env);
    }
});
