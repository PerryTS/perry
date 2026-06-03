#![cfg(feature = "container")]
use perry_stdlib::container::capability::*;
use std::collections::HashMap;

// Feature: perry-container | Layer: unit | Req: 13.1 | Property: -
#[test]
fn test_capability_grants_struct() {
    let mut env = HashMap::new();
    env.insert("FOO".into(), "BAR".into());
    let grants = CapabilityGrants {
        network: true,
        env: Some(env),
    };
    assert!(grants.network);
    assert_eq!(grants.env.unwrap().get("FOO").unwrap(), "BAR");
}

// Coverage Table:
// | Requirement | Test name | Layer |
// |-------------|-----------|-------|
// | 13.1        | test_capability_grants_struct | unit |

// Deferred Requirements:
// Req 13.2-13.5 - Running capabilities requires a functioning OCI backend and image verification.
