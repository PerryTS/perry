//! Tests for `SecurityProfile::merge_security_opt` — pinning the
//! v0.5.380 fix where `security_opt: ["seccomp=...", "no-new-privileges"]`
//! on a `ComposeService` was silently dropped on its way to the runtime.
//!
//! Pre-fix users got the looser default while their spec said hardened.
//! These tests are the canary: if a future refactor regresses this path,
//! exactly one named test fails and points at the dropped flag.

use perry_container_compose::backend::{DockerProtocol, SecurityProfile};
use perry_container_compose::CliProtocol;

#[test]
fn merge_seccomp_path() {
    let mut p = SecurityProfile::default();
    p.merge_security_opt(&["seccomp=/etc/strict.json".into()]);
    assert_eq!(p.seccomp, Some("/etc/strict.json".into()));
}

#[test]
fn merge_seccomp_default() {
    let mut p = SecurityProfile::default();
    p.merge_security_opt(&["seccomp=default".into()]);
    assert_eq!(p.seccomp, Some("default".into()));
}

#[test]
fn merge_seccomp_colon_form() {
    // Compose-spec accepts both `=` and `:` separators; the parser
    // handles both so users porting from various dockerfile dialects
    // don't trip on syntax.
    let mut p = SecurityProfile::default();
    p.merge_security_opt(&["seccomp:/etc/strict.json".into()]);
    assert_eq!(p.seccomp, Some("/etc/strict.json".into()));
}

#[test]
fn merge_no_new_privileges_bare() {
    let mut p = SecurityProfile::default();
    p.merge_security_opt(&["no-new-privileges".into()]);
    assert!(p.no_new_privileges);
}

#[test]
fn merge_no_new_privileges_colon_true() {
    let mut p = SecurityProfile::default();
    p.merge_security_opt(&["no-new-privileges:true".into()]);
    assert!(p.no_new_privileges);
}

#[test]
fn merge_no_new_privileges_equals_true() {
    let mut p = SecurityProfile::default();
    p.merge_security_opt(&["no-new-privileges=true".into()]);
    assert!(p.no_new_privileges);
}

#[test]
fn merge_combined() {
    // The realistic case: user specifies both flags at once.
    let mut p = SecurityProfile::default();
    p.merge_security_opt(&[
        "seccomp=/etc/strict.json".into(),
        "no-new-privileges:true".into(),
    ]);
    assert_eq!(p.seccomp, Some("/etc/strict.json".into()));
    assert!(p.no_new_privileges);
}

#[test]
fn merge_unknown_opts_ignored() {
    // Defensive: unrecognised entries don't break the parser. The
    // future-extension story (label-mode=disable, apparmor=...) is
    // additive — adding new arms in the parser without breaking
    // existing callers.
    let mut p = SecurityProfile::default();
    p.merge_security_opt(&["unknown-opt".into(), "label-disable".into()]);
    assert_eq!(p.seccomp, None);
    assert!(!p.no_new_privileges);
}

#[test]
fn docker_security_args_emit_no_new_privileges_when_set() {
    // The full pipe — parser → SecurityProfile → DockerProtocol's
    // `security_args()` → CLI flag emission. Pin that the v0.5.380
    // fix is end-to-end (parser-only fix without the emitter would
    // still drop the flag at the CLI boundary).
    let proto = DockerProtocol;
    let profile = SecurityProfile {
        no_new_privileges: true,
        ..Default::default()
    };
    let args = proto.security_args(&profile);
    let pairs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    assert!(
        pairs
            .windows(2)
            .any(|w| w[0] == "--security-opt" && w[1].starts_with("no-new-privileges")),
        "DockerProtocol must emit `--security-opt no-new-privileges:...` when set; got {:?}",
        args
    );
}

#[test]
fn docker_security_args_emit_seccomp_when_set() {
    let proto = DockerProtocol;
    let profile = SecurityProfile {
        seccomp: Some("/etc/strict.json".into()),
        ..Default::default()
    };
    let args = proto.security_args(&profile);
    let pairs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    assert!(
        pairs
            .windows(2)
            .any(|w| w[0] == "--security-opt" && w[1] == "seccomp=/etc/strict.json"),
        "DockerProtocol must emit `--security-opt seccomp=/etc/strict.json`; got {:?}",
        args
    );
}

#[test]
fn docker_security_args_empty_for_default_profile() {
    // The opposite canary: no security flags set → no emitted args.
    // Pin that we don't emit garbage when the user didn't ask for any.
    let proto = DockerProtocol;
    let profile = SecurityProfile::default();
    let args = proto.security_args(&profile);
    assert!(
        args.is_empty(),
        "Default profile must produce no args; got {:?}",
        args
    );
}
