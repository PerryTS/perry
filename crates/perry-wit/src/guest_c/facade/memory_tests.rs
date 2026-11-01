use super::*;

#[test]
fn forwarding_frees_consumed_input_buffers_before_kick_and_preserves_the_result() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.wit");
    std::fs::write(
        &path,
        r#"package test:facade-memory;
        interface api { resource item; }
        world plugin {
            import api; use api.{item};
            record payload { item: own<item>, text: string, values: list<string> }
            export echo: func(value: payload) -> string;
        }"#,
    )
    .unwrap();
    // The fixture needs no async public signature, but uses the experimental
    // entry point to exercise the same facade as real producer components.
    let files = experimental_async_guest(&path, None).unwrap();
    let plan: Plan = serde_json::from_slice(&files["perry_producer_facade.json"]).unwrap();
    let original = String::from_utf8(files["perry_producer_forward.c"].clone()).unwrap();
    let declaration = original
        .lines()
        .find(|line| line.starts_with("void exports_") && line.contains("arg0"))
        .unwrap();
    let forward = declaration
        .strip_prefix("void ")
        .unwrap()
        .split('(')
        .next()
        .unwrap();
    let input = declaration
        .split('(')
        .nth(1)
        .unwrap()
        .split(" *arg0")
        .next()
        .unwrap();
    let native_type = original
        .lines()
        .find_map(|line| line.trim().strip_suffix(" native0;"))
        .unwrap();
    let native = format!(
        "plugin_{}",
        plan.imports
            .iter()
            .find(|i| i.original == "echo")
            .unwrap()
            .local
            .replace('-', "_")
    );
    let kick = format!(
        "plugin_{}",
        plan.imports
            .iter()
            .find(|i| i.original == plan.actor)
            .unwrap()
            .local
            .replace('-', "_")
    );
    let header = String::from_utf8(files["perry_producer_facade.h"].clone()).unwrap();
    let signature = header
        .lines()
        .find(|line| line.contains(&format!("{native}(")))
        .unwrap()
        .trim()
        .trim_end_matches(';');
    let parameters = signature
        .split('(')
        .nth(1)
        .unwrap()
        .trim_end_matches(')')
        .split(',')
        .map(|p| {
            p.split_whitespace()
                .last()
                .unwrap()
                .trim_start_matches('*')
                .to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(parameters.len(), 2);
    let mut harness = include_str!("memory.c").to_string();
    for (key, value) in [
        ("$FORWARD", forward),
        ("$INPUT", input),
        ("$NATIVE_TYPE", native_type),
        ("$SIGNATURE", signature),
        ("$VALUE", &parameters[0]),
        ("$RETURN", &parameters[1]),
        ("$KICK", &kick),
    ] {
        harness = harness.replace(key, value);
    }
    for (name, bytes) in files {
        std::fs::write(dir.path().join(name), bytes).unwrap();
    }
    let source = dir.path().join("memory.c");
    std::fs::write(&source, harness).unwrap();
    for negative in [false, true] {
        if negative {
            let call = format!(
                "{}_free(&native0);",
                native_type.strip_suffix("_t").unwrap()
            );
            assert!(original.contains(&call));
            std::fs::write(
                dir.path().join("perry_producer_forward.c"),
                original.replace(&call, ""),
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
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains("freed_buffers"));
        } else {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout)
                .contains("PASS: typed facade input retirement and result custody"));
        }
    }
}
