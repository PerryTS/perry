use super::*;

const CUSTODY_WORLD: &str = r#"package test:custody;
interface api {
    resource item;
    type owners = list<own<item>>;
    record leaf { first: own<item>, second: own<item>, label: string }
    record request { owners: list<leaf>, borrowed: borrow<item>, fallback: option<own<item>> }
    variant reply { nothing, held(tuple<own<item>, option<owners>>), text(string) }
    take: async func(value: request, other: own<item>, padding: u64, more: u64)
        -> result<reply, own<item>>;
    one: async func(value: own<item>) -> own<item>;
}
world plugin { import api; export run: async func(); }
"#;

// Exercise the actual experimental generator, while ensuring the public CLI
// still rejects async output. No resource-specific guard is bypassed here.
fn custody_files(source: &str) -> BTreeMap<String, Vec<u8>> {
    custody_world_files(source, None)
}

fn custody_world_files(source: &str, selected: Option<&str>) -> BTreeMap<String, Vec<u8>> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.wit");
    std::fs::write(&path, source).unwrap();
    assert!(canonical_guest(&path, selected)
        .unwrap_err()
        .to_string()
        .contains("async guest functions"));
    experimental_async_guest(&path, selected).unwrap()
}

/// Produce a real resource-bearing guest through the experimental entry point.
/// The external acceptance harness compiles/executes it with matching tools;
/// generating these files alone is never a passing component qualification.
#[test]
#[ignore = "fixture producer for source-matched WASI resource qualification"]
fn async_resource_guest_fixture() {
    resource_fixture("async-guest-resources", None);
}

#[test]
#[ignore = "fixture producer for source-matched compound resource qualification"]
fn async_compound_resource_guest_fixture() {
    resource_fixture("async-compound-resources", Some("plugin"));
}

fn resource_fixture(name: &str, selected: Option<&str>) {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-wasi/components")
        .join(name);
    let source = std::fs::read_to_string(fixture.join("world.wit")).unwrap();
    let files = custody_world_files(&source, selected);
    let output = tempfile::tempdir().unwrap().keep();
    let generated = output.join("generated");
    std::fs::create_dir(&generated).unwrap();
    for (name, bytes) in files {
        std::fs::write(generated.join(name), bytes).unwrap();
    }
    std::fs::write(
        generated.join("guest.ts"),
        crate::typescript_guest(&crate::parse(&source, selected).unwrap()),
    )
    .unwrap();
    for name in ["world.wit", "guest.ts"] {
        std::fs::copy(fixture.join(name), output.join(name)).unwrap();
    }
    println!("PERRY_ASYNC_RESOURCE_FIXTURE={}", output.display());
}

#[test]
fn experimental_async_resource_cleanup_compiles_with_imported_and_local_handles() {
    for source in [
        CUSTODY_WORLD.to_string(),
        // An exported interface retains imported resource identity, while its
        // locally defined resource must use its own canonical drop function.
        CUSTODY_WORLD.replace(
            "world plugin { import api; export run: async func(); }",
            "interface local { use api.{item}; resource gadget; recycle: async func(a: own<gadget>, b: own<item>) -> own<gadget>; } world plugin { import api; export local; export run: async func(); }",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in custody_files(&source) {
            std::fs::write(dir.path().join(name), bytes).unwrap();
        }
        let output = std::process::Command::new("clang")
            .args(["-std=c11", "-Wall", "-Werror", "-Wno-unused-function", "-fsyntax-only"])
            .arg(dir.path().join("perry_adapter.c"))
            .output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn canonical_child_ownership_survives_cancel_acknowledgement_and_return_races() {
    let dir = tempfile::tempdir().unwrap();
    let files = custody_files(CUSTODY_WORLD);
    let adapter = String::from_utf8(files["perry_adapter.c"].clone()).unwrap();
    let free = adapter
        .lines()
        .find_map(|line| {
            line.strip_suffix("_free(&data->result);")
                .map(|stem| format!("{stem}_free"))
        })
        .unwrap();
    for (name, bytes) in files {
        std::fs::write(dir.path().join(name), bytes).unwrap();
    }
    let harness = include_str!("tests/custody.c").replace("$RESULT_FREE", &free);
    let source = dir.path().join("custody.c");
    std::fs::write(&source, harness).unwrap();
    let exe = dir.path().join("custody");
    let output = std::process::Command::new("clang")
        .args([
            "-std=c11",
            "-Wall",
            "-Werror",
            "-Wno-unused-function",
            "-ffunction-sections",
            "-fdata-sections",
            "-Wl,--gc-sections",
            // Avoid retaining the unused adapter registration graph through ASan
            // global metadata. Heap/stack/use-after-free checks remain enabled.
            "-fsanitize=address,undefined",
            "-mllvm",
            "-asan-globals=0",
            "-fno-omit-frame-pointer",
            "-g",
            "-O1",
        ])
        .arg(&source)
        .arg("-o")
        .arg(&exe)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = std::process::Command::new(&exe)
        .env("ASAN_OPTIONS", "detect_leaks=1:halt_on_error=1")
        .env("UBSAN_OPTIONS", "halt_on_error=1:print_stacktrace=1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("PASS: 14 canonical child ownership cases"));
    // This invokes the real generated simple-resource finish callback. The
    // native trap is SIGILL; the settlement mock would instead abort (SIGABRT).
    use std::os::unix::process::ExitStatusExt;
    let output = std::process::Command::new(&exe)
        .arg("--failed-owned-lift")
        .env("ASAN_OPTIONS", "handle_sigill=0")
        .output()
        .unwrap();
    assert_eq!(
        output.status.signal(),
        Some(4),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(target_os = "linux")]
#[test]
fn async_export_commits_owned_results_and_retires_borrows_before_canonical_return() {
    let source = "package test:return-resource; interface api { resource item; } world plugin { import api; use api.{item}; export give: async func() -> own<item>; }";
    let dir = tempfile::tempdir().unwrap();
    let files = custody_files(source);
    let adapter = String::from_utf8(files["perry_adapter.c"].clone()).unwrap();
    let return_function = adapter
        .lines()
        .find_map(|line| {
            line.strip_suffix("(data.result);")
                .filter(|name| name.ends_with("_return"))
        })
        .unwrap();
    for (name, bytes) in files {
        std::fs::write(dir.path().join(name), bytes).unwrap();
    }
    let harness =
        include_str!("tests/export_resource.c").replace("$RETURN_FUNCTION", return_function);
    let path = dir.path().join("export_resource.c");
    std::fs::write(&path, harness).unwrap();
    for negative in [false, true] {
        if negative {
            std::fs::write(
                dir.path().join("perry_adapter.c"),
                adapter.replace("perry_component_frame_resource_commit(data.frame);", ""),
            )
            .unwrap();
        }
        let exe = dir
            .path()
            .join(if negative { "negative" } else { "positive" });
        let output = std::process::Command::new("clang")
            .args([
                "-std=c11",
                "-Wall",
                "-Werror",
                "-Wno-unused-function",
                "-ffunction-sections",
                "-fdata-sections",
                "-Wl,--gc-sections",
                "-O1",
            ])
            .arg(&path)
            .arg("-o")
            .arg(&exe)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = std::process::Command::new(&exe).output().unwrap();
        if negative {
            assert!(
                !output.status.success(),
                "missing ownership commit must fail"
            );
            assert!(String::from_utf8_lossy(&output.stderr).contains("committed"));
        } else {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("PASS: async export commits"));
        }
    }
}
