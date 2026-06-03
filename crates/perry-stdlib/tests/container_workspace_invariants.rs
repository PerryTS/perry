#![cfg(feature = "container")]
// Workspace-invariant tests for the `perry/container` subsystem.
//
// These don't exercise the runtime — they assert structural properties
// of the workspace itself. The container subsystem requires three
// independent things to all be present, and the file with each one is
// frequently auto-edited by tooling that strips "extra" entries. When
// any of the three is missing, the build fails with confusing errors
// downstream (e.g. "perry-container-compose: package ID specification
// did not match any packages"). These tests catch the missing entry
// upstream with a clear error message instead.
#![cfg(feature = "container")]

use std::path::PathBuf;

fn workspace_cargo_toml() -> String {
    // tests run from the crate's CARGO_MANIFEST_DIR; walk up until we
    // find the workspace root. We need a stricter check than just
    // `contains("[workspace]")` because stdlib's own Cargo.toml has
    // that substring inside a comment block — the workspace root
    // additionally has `members = [` after the `[workspace]` header.
    let mut p: PathBuf = env!("CARGO_MANIFEST_DIR").into();
    loop {
        let candidate = p.join("Cargo.toml");
        if candidate.exists() {
            let s = std::fs::read_to_string(&candidate).expect("read Cargo.toml");
            if s.lines().any(|line| line.trim_start() == "[workspace]") && s.contains("members = [")
            {
                return s;
            }
        }
        if !p.pop() {
            panic!(
                "could not find workspace Cargo.toml above {}",
                env!("CARGO_MANIFEST_DIR")
            );
        }
    }
}

#[test]
fn perry_container_compose_in_workspace_members() {
    let toml = workspace_cargo_toml();
    assert!(
        toml.contains("\"crates/perry-container-compose\""),
        "perry-container-compose missing from [workspace] members in workspace Cargo.toml — \
         the container feature can't build without it. Re-add `\"crates/perry-container-compose\"` \
         to the `members = [...]` array. Likely cause: a tool stripped \"extra\" entries on save."
    );
}

#[test]
fn perry_container_compose_in_default_members() {
    let toml = workspace_cargo_toml();
    // Locate `default-members = [` block and check for the entry inside.
    let start = toml
        .find("default-members = [")
        .expect("default-members block not found in workspace Cargo.toml");
    let block = &toml[start..];
    let end = block.find(']').expect("default-members not closed");
    let block = &block[..=end];
    assert!(
        block.contains("\"crates/perry-container-compose\""),
        "perry-container-compose missing from [workspace] default-members. Without it \
         `cargo build` (no `-p`) won't build the crate, breaking auto-optimize for users \
         who import `perry/container`. Re-add `\"crates/perry-container-compose\"` to \
         `default-members = [...]`."
    );
}
