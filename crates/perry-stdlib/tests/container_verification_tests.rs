#![cfg(feature = "container")]
use perry_stdlib::container::verification::*;
use tokio;

// Feature: perry-container | Layer: unit | Req: 15.4 | Property: 10
#[tokio::test]
async fn test_get_chainguard_image() {
    assert_eq!(
        get_chainguard_image("git").unwrap(),
        "cgr.dev/chainguard/git"
    );
    assert_eq!(
        get_chainguard_image("python").unwrap(),
        "cgr.dev/chainguard/python"
    );
    assert!(get_chainguard_image("unknown-tool").is_none());
}

// Feature: perry-container | Layer: unit | Req: 14.1 | Property: -
#[test]
fn test_get_default_base_image() {
    assert_eq!(get_default_base_image(), "cgr.dev/chainguard/alpine-base");
}

// Coverage Table:
// | Requirement | Test name | Layer |
// |-------------|-----------|-------|
// | 14.1        | test_get_default_base_image | unit |
// | 15.4        | test_get_chainguard_image | unit |

// Deferred Requirements:
// Req 15.1, 15.2, 15.3, 15.5, 15.7 - Image verification requires live network and cosign/crane binaries.
