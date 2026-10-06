//! #12023: supported targets always use statepoints, including long-lived
//! native homes. Removed environment selectors cannot change root coverage.
//! Both builds run every moving-GC configuration and must agree.
use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Keep caller roots alive across allocating callee calls and collection.
const SOURCE: &str = r#"
function leaf(o: { v: number }): number {
  return o.v;
}

function make(i: number): { v: number } {
  return { v: i };
}

function run(): number {
  let acc = 0;
  const keep: { v: number }[] = [];
  for (let i = 0; i < 40000; i++) {
    const a = make(i);
    const b = make(i * 2);
    acc = (acc + leaf(a) + leaf(b)) | 0;
    if (i % 7 === 0) keep.push(a);
    if (keep.length > 128) keep.shift();
  }
  let s = 0;
  for (const k of keep) s = (s + leaf(k)) | 0;
  return (acc + s) | 0;
}

console.log("r:" + run());
"#;

/// Collector knobs cleared before each run so a developer's exported kill
/// switch cannot turn every arm into the never-relocates control (mirrors
/// `gc_closure_self_pointer_root_7055`).
const GC_ENV_OVERRIDES: &[&str] = &[
    "PERRY_GEN_GC",
    "PERRY_GC_SCAVENGE",
    "PERRY_GC_SCAVENGE_NURSERY_MB",
    "PERRY_GC_MOVING_SAFEPOINT",
    "PERRY_GC_MOVING_LOOP_POLLS",
    "PERRY_GC_FORCE_EVACUATE",
    "PERRY_CONSERVATIVE_STACK_SCAN",
    "PERRY_WRITE_BARRIERS",
    "PERRY_GC_INCREMENTAL",
    "PERRY_GC_HEAP_LIMIT",
];

fn compile(dir: &std::path::Path, spill_threshold: &str) -> (PathBuf, String) {
    let entry = dir.join("main.ts");
    let output = dir.join(format!("bin_spill_{spill_threshold}"));
    std::fs::write(&entry, SOURCE).expect("write entry");
    let ir = dir.join(format!("ir_{spill_threshold}"));
    std::fs::create_dir_all(&ir).unwrap();
    let mut command = Command::new(perry_bin());
    if spill_threshold == "1" {
        command
            .env("PERRY_RS4GC", "0")
            .env("PERRY_SHADOW_STACK", "0")
            .env("PERRY_INLINE_SHADOW_SLOT", "0");
    }
    let out = command
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .arg("--no-auto-optimize")
        .env("PERRY_SAVE_LL", &ir)
        .env("PERRY_ROOT_SPILL_RELOCATIONS", spill_threshold)
        .output()
        .expect("run perry compile");
    assert!(
        out.status.success(),
        "perry compile (PERRY_ROOT_SPILL_RELOCATIONS={spill_threshold}) failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let mut files: Vec<_> = std::fs::read_dir(ir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "ll"))
        .collect();
    files.sort();
    let text = files
        .into_iter()
        .map(|path| std::fs::read_to_string(path).unwrap())
        .collect::<String>();
    assert!(
        text.contains("gc \"statepoint-example\""),
        "native rooting must be visible in saved IR"
    );
    for call in [
        "call ptr @js_shadow_frame_enter",
        "call i64 @js_shadow_frame_push",
        "call void @js_shadow_slot_bind",
        "call void @js_shadow_slot_set",
    ] {
        assert!(
            !text.contains(call),
            "native IR retained shadow traffic: {call}"
        );
    }
    (output, text)
}

fn run_arms(binary: &std::path::Path, dir: &std::path::Path, label: &str) -> String {
    let mut arms: Vec<Vec<(&str, &str)>> = vec![vec![]];
    for mb in ["1", "2", "4", "8"] {
        arms.push(vec![("PERRY_GC_SCAVENGE_NURSERY_MB", mb)]);
    }
    arms.push(vec![("PERRY_GEN_GC", "0")]);

    arms.extend([
        vec![("PERRY_GC_FORCE_EVACUATE", "1")],
        vec![
            ("PERRY_GC_FORCE_EVACUATE", "1"),
            ("PERRY_GC_VERIFY_EVACUATION", "1"),
            ("PERRY_GC_VERIFY_MARK", "1"),
        ],
        vec![
            ("PERRY_GC_MOVING_SAFEPOINT", "1"),
            ("PERRY_GC_SCHEDULE_ALLOC_KB", "64"),
        ],
        vec![("PERRY_GC_BUDGETED_OLD_RECLAIM", "1")],
    ]);
    let mut first: Option<String> = None;
    for arm in &arms {
        let mut cmd = Command::new(binary);
        cmd.current_dir(dir);
        for key in GC_ENV_OVERRIDES {
            cmd.env_remove(key);
        }
        for (k, v) in arm {
            cmd.env(k, v);
        }
        let run = cmd.output().expect("run compiled binary");
        let arm_label = if arm.is_empty() {
            format!("{label}/default")
        } else {
            format!(
                "{label}/{}",
                arm.iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };
        assert!(
            run.status.success(),
            "[{arm_label}] compiled binary failed (exit {:?})\nstderr:\n{}",
            run.status.code(),
            String::from_utf8_lossy(&run.stderr),
        );
        let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
        match &first {
            None => first = Some(stdout),
            Some(f) => assert_eq!(
                &stdout, f,
                "[{arm_label}] output differs between collector arms — a moving \
                 minor left a stale root in this configuration"
            ),
        }
    }
    first.expect("at least one arm ran")
}

#[test]
fn removed_rooting_selectors_cannot_disable_statepoints() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (native_bin, native_ir) = compile(dir.path(), "0");
    let (ignored_bin, ignored_ir) = compile(dir.path(), "1");
    assert_eq!(
        native_ir, ignored_ir,
        "removed selectors changed the native-root IR"
    );
    let native_out = run_arms(&native_bin, dir.path(), "native");
    let ignored_out = run_arms(&ignored_bin, dir.path(), "removed selectors");
    assert!(
        native_out.starts_with("r:"),
        "unexpected program output: {native_out:?}"
    );
    assert_eq!(
        native_out, ignored_out,
        "a root was lost or not rewritten under moving GC"
    );
}
