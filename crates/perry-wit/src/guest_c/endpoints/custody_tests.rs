use super::*;

#[test]
fn canonical_guest_reads_retire_only_acknowledged_unclaimed_owned_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.wit");
    std::fs::write(
        &path,
        r#"package test:endpoint-custody;
        interface api {
            resource item;
            record payload { item: own<item>, text: string }
            type pending = future<payload>;
            type values = stream<payload>;
        }
        world plugin {
            import api;
            use api.{pending, values};
            export run: async func(value: pending, values: values);
        }"#,
    )
    .unwrap();
    assert!(canonical_guest(&path, None)
        .unwrap_err()
        .to_string()
        .contains("future/stream"));
    let files = experimental_async_guest(&path, None).unwrap();
    let adapter = String::from_utf8(files["perry_adapter.c"].clone()).unwrap();
    // Discover generated names, then exercise the actual generated callbacks
    // and scheduler. No copy of their ownership algorithm lives in the test.
    let mut harness = include_str!("tests/custody.c").to_string();
    let mut kinds = Vec::new();
    for declaration in adapter
        .lines()
        .filter(|line| line.contains("payload_t *items; } perry_read_"))
    {
        let stem = declaration
            .split("} ")
            .nth(1)
            .unwrap()
            .strip_suffix("_state;")
            .unwrap();
        let body = adapter
            .split(&format!("static int32_t {stem}_start("))
            .nth(1)
            .unwrap()
            .split("\nreturn 0;\n}")
            .next()
            .unwrap();
        let kind = if body.contains("PLUGIN_EVENT_FUTURE_READ") {
            "FUTURE"
        } else {
            assert!(body.contains("PLUGIN_EVENT_STREAM_READ"));
            "STREAM"
        };
        let helper = body
            .lines()
            .find_map(|line| line.strip_prefix("io->status = "))
            .unwrap()
            .split('(')
            .next()
            .unwrap()
            .strip_suffix("_read")
            .unwrap();
        harness = harness
            .replace(&format!("${kind}_STEM"), stem)
            .replace(&format!("${kind}_HELPER"), helper);
        kinds.push(kind);
    }
    kinds.sort();
    assert_eq!(kinds, ["FUTURE", "STREAM"]);
    for (name, bytes) in files {
        std::fs::write(dir.path().join(name), bytes).unwrap();
    }
    let source = dir.path().join("custody.c");
    std::fs::write(&source, harness).unwrap();
    for negative in [false, true] {
        if negative {
            assert!(adapter.contains("if (!io->claimed)"));
            std::fs::write(
                dir.path().join("perry_adapter.c"),
                adapter.replace("if (!io->claimed)", "if (true)"),
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
                "dropping claimed owners must fail"
            );
            assert!(String::from_utf8_lossy(&output.stderr).contains("adopted"));
        } else {
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout)
                .contains("PASS: 26 guest read ownership cases"));
            // Future read results are uncounted enum codes. A packed stream
            // count or DROPPED code must trap before any payload is lifted.
            use std::os::unix::process::ExitStatusExt;
            for argument in ["--packed-future-count", "--dropped-future-read"] {
                let output = std::process::Command::new(&exe)
                    .arg(argument)
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
}
