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

// Deliberately exercise code below the resource profile guard. There is no
// public or experimental entry point which bypasses that guard.
fn custody_files(source: &str) -> BTreeMap<String, Vec<u8>> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.wit");
    std::fs::write(&path, source).unwrap();
    assert!(experimental_async_guest(&path, None)
        .unwrap_err()
        .to_string()
        .contains("resources"));
    let mut resolve = Resolve::default();
    let (package, _) = resolve.push_path(&path).unwrap();
    let id = resolve.select_world(&[package], None).unwrap();
    resolve.generate_nominal_type_ids(id);
    let world = crate::resolve::convert(&resolve, id).unwrap();
    let anchor = add_wake_anchor(&mut resolve, id).unwrap();
    generate_resolved(resolve, id, world, Some(anchor)).unwrap()
}

#[test]
fn guarded_async_resource_cleanup_compiles_with_imported_and_local_handles() {
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
