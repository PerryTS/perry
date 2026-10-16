//! A layout descriptor's `_id` alias (`layout_desc`) must sit in the codegen
//! unit that defines the descriptor. An array named-property layout's alias
//! is loaded by nothing, so the split kept it in unit 0 while the descriptor
//! went to the unit attaching the layout, and LLVM rejected unit 0 with
//! "Alias must point to a definition" (three Next.js server bundles).
//!
//! Every gap fixture compiles as one unit; `PERRY_CODEGEN_UNITS=2` forces the
//! split here.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            perry_bin()
                .parent()
                .expect("compiler directory")
                .to_path_buf()
        })
}

/// `node --experimental-strip-types` output of the fixture.
const EXPECTED: &str = "3 4 6 alpha 8\n3 b 3 a,b,c\n60980\n";

#[test]
fn array_layout_id_alias_lives_with_its_descriptor_in_a_split_module() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-files/test_gap_array_layout_multi_unit.ts");
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::copy(&fixture, &entry).expect("copy fixture");
    let output = dir.path().join("main_bin");
    let ll_dir = dir.path().join("ll");
    std::fs::create_dir(&ll_dir).expect("create IR dump dir");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_CODEGEN_UNITS", "2")
        .env_remove("PERRY_LLVM_INPROCESS")
        .env("PERRY_SAVE_LL", &ll_dir)
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    // The subject must be live: both units exist and one of them defines an
    // array layout id alias.
    let units: Vec<String> = (0..2)
        .map(|unit| {
            let path = ll_dir.join(format!("main_ts.unit{unit}.native.ll"));
            std::fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("the module was not split: {}", path.display()))
        })
        .collect();
    let aliases: Vec<(usize, &str)> = units
        .iter()
        .enumerate()
        .flat_map(|(i, unit)| unit.lines().map(move |line| (i, line)))
        .filter(|(_, line)| line.starts_with("@perry_array_layout_") && line.contains(" alias "))
        .collect();
    assert!(!aliases.is_empty(), "no array layout id alias was emitted");
    for (unit, line) in aliases {
        let aliasee = line
            .split("ptr @")
            .nth(1)
            .and_then(|tail| tail.split(|c: char| c == ',' || c == ')').next())
            .expect("alias names its descriptor");
        let defines = units[unit].lines().any(|l| {
            l.strip_prefix(&format!("@{aliasee} = "))
                .is_some_and(|rhs| !rhs.starts_with("external "))
        });
        assert!(
            defines,
            "unit {unit} defines `{line}` without defining @{aliasee}"
        );
    }

    let run = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), EXPECTED);
}
