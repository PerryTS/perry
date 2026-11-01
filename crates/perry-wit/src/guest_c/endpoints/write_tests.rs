use super::*;

#[test]
fn canonical_guest_writes_recover_only_unconsumed_owners_after_native_acknowledgement() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.wit");
    std::fs::write(&path, r#"package test:writer-custody;
        interface api {
            resource item;
            record payload { item: own<item>, text: string }
            type pending = future<payload>;
            type values = stream<payload>;
        }
        world plugin { import api; use api.{pending, values}; export run: async func(a: pending, b: values); }
    "#).unwrap();
    assert!(canonical_guest(&path, None).is_err());
    let files = experimental_async_guest(&path, None).unwrap();
    let adapter = String::from_utf8(files["perry_adapter.c"].clone()).unwrap();
    let mut harness = include_str!("tests/write.c").to_string();
    let mut kinds = Vec::new();
    for signature in adapter
        .lines()
        .filter(|line| line.starts_with("static int32_t perry_write_") && line.contains("_start("))
    {
        let stem = signature
            .strip_prefix("static int32_t ")
            .unwrap()
            .split("_start(")
            .next()
            .unwrap();
        let body = adapter
            .split(&format!("static int32_t {stem}_start("))
            .nth(1)
            .unwrap()
            .split("\nstatic uint32_t perry_new_")
            .next()
            .unwrap();
        let kind = if body.contains("PLUGIN_EVENT_FUTURE_WRITE") {
            "FUTURE"
        } else {
            assert!(body.contains("PLUGIN_EVENT_STREAM_WRITE"));
            "STREAM"
        };
        let helper = body
            .lines()
            .find_map(|line| line.trim().strip_prefix("io->status = "))
            .unwrap()
            .split('(')
            .next()
            .unwrap()
            .strip_suffix("_write")
            .unwrap();
        harness = harness
            .replace(&format!("{}{kind}_STEM", '$'), stem)
            .replace(&format!("{}{kind}_HELPER", '$'), helper)
            .replace(
                &format!("{}{kind}_TYPE", '$'),
                stem.strip_prefix("perry_write_").unwrap(),
            );
        kinds.push(kind);
    }
    kinds.sort();
    assert_eq!(kinds, ["FUTURE", "STREAM"]);
    for (name, bytes) in files {
        std::fs::write(dir.path().join(name), bytes).unwrap();
    }
    let source = dir.path().join("write.c");
    std::fs::write(&source, harness).unwrap();
    for negative in [false, true] {
        if negative {
            assert!(adapter.contains("index = data->sent"));
            std::fs::write(
                dir.path().join("perry_adapter.c"),
                adapter.replace("index = data->sent", "index = 0"),
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
        if negative {
            assert!(
                !output.status.success(),
                "dropping the consumed prefix must fail"
            );
            assert!(String::from_utf8_lossy(&output.stderr).contains("sent_handles"));
        } else {
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout)
                .contains("PASS: 29 guest write custody cases"));
            use std::os::unix::process::ExitStatusExt;
            let output = std::process::Command::new(&exe)
                .arg("--unfinished-future")
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
    }
}
