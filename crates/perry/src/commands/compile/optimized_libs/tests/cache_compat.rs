use super::*;

pub(super) fn matching_runtime_archive() -> Vec<u8> {
    format!(
        "!<arch>\nPERRY_RUNTIME_BUILD_STAMP_V1|version={}|build={}\0",
        env!("CARGO_PKG_VERSION"),
        perry_runtime::PERRY_RUNTIME_BUILD_ID,
    )
    .into_bytes()
}

#[test]
fn auto_optimized_freshness_rejects_incompatible_embedded_stamp() {
    let dir = tempfile::tempdir().expect("tempdir");
    minimal_auto_workspace(dir.path());
    let runtime = dir.path().join("target/runtime.a");
    let stdlib = dir.path().join("target/stdlib.a");
    let stamp = dir.path().join("target/.perry-auto-build.stamp");
    write_file(&stdlib, b"!<arch>\n");
    write_file(&stamp, b"same-source-and-options");
    for contents in [
        b"!<arch>\n".as_slice(),
        b"PERRY_RUNTIME_BUILD_STAMP_V1|version=0.0.0|build=src:stale\0",
        b"PERRY_RUNTIME_BUILD_STAMP_V1|broken\0",
    ] {
        write_file(&runtime, contents);
        assert!(
            !auto_optimized_archives_are_fresh(
                dir.path(),
                &runtime,
                &stdlib,
                &[],
                &stamp,
                "same-source-and-options",
            ),
            "an archive rejected at link must never be fresh"
        );
    }
    write_file(&runtime, &matching_runtime_archive());
    assert!(auto_optimized_archives_are_fresh(
        dir.path(),
        &runtime,
        &stdlib,
        &[],
        &stamp,
        "same-source-and-options",
    ));
}
