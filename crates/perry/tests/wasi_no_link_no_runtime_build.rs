//! Compile-only WASI must not try to build archives for a link it will skip.
#![cfg(all(unix, feature = "target-wasi"))]

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn wasi_no_link_emits_ir_and_an_object_without_invoking_cargo() {
    let work = tempfile::tempdir().unwrap();
    let shim = work.path().join("bin");
    std::fs::create_dir(&shim).unwrap();
    let cargo = shim.join("cargo");
    std::fs::write(&cargo, "#!/bin/sh\ntouch cargo-was-invoked\nexit 97\n").unwrap();
    std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        work.path().join("main.ts"),
        "import { createGzip } from 'node:zlib'; console.log(typeof createGzip());\n",
    )
    .unwrap();
    let path = std::env::join_paths(
        std::iter::once(shim).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(work.path())
        .args([
            "compile",
            "main.ts",
            "--target",
            "wasi",
            "--no-link",
            "--no-auto-optimize",
            "--no-cache",
            "--trace",
            "llvm",
            "-o",
            "main.o",
        ])
        .env("PATH", path)
        .output()
        .expect("run compile-only WASI");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !work.path().join("cargo-was-invoked").exists(),
        "--no-link must not build runtime/extension archives"
    );
    let object = std::fs::read(work.path().join("main.o")).unwrap();
    assert!(
        object.starts_with(b"\0asm"),
        "a WASI object must actually be emitted"
    );
    assert!(
        std::fs::read_dir(work.path().join(".perry-trace/llvm"))
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry.path().extension().is_some_and(|x| x == "ll")),
        "the GC-root gate must receive its real IR"
    );
}
