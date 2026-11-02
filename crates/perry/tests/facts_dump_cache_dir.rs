//! The facts dump of a program is the same bytes whichever `--cache-dir` it
//! was compiled with. A CommonJS program compiles a generated support module
//! under `<cache dir>/synthetic-modules`; its module name used to carry the
//! cache dir into every dump line naming it.

use std::path::{Path, PathBuf};
use std::process::Command;

fn dump(dir: &Path, cache: &str) -> Vec<u8> {
    let cache_dir = dir.join(cache);
    let dump = dir.join(format!("dump_{cache}"));
    let mut cmd = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_perry")));
    for (key, _) in std::env::vars() {
        if key.starts_with("PERRY_") || key == "NODE_OPTIONS" {
            cmd.env_remove(key);
        }
    }
    let out = cmd
        .current_dir(dir)
        .env("PERRY_FACTS_DUMP", &dump)
        .env("PERRY_FACTS_ONLY", "1")
        .arg("compile")
        .arg("main.ts")
        .arg("-o")
        .arg(dir.join(format!("main_{cache}")))
        .arg("--cache-dir")
        .arg(&cache_dir)
        .output()
        .expect("run perry");
    assert!(
        out.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // Timings are the one part of a dump that varies between runs.
    std::fs::read_to_string(&dump)
        .expect("facts dump")
        .lines()
        // `W` lines name files beside the dump itself, not under the cache dir.
        .filter(|l| !l.contains("_ms") && !l.contains(" ms=") && !l.starts_with("W "))
        .map(|l| format!("{l}\n"))
        .collect::<String>()
        .into_bytes()
}

#[test]
fn two_cache_dirs_dump_the_same_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    std::fs::write(
        dir.join("main.ts"),
        "const m = require(\"./a.cjs\");\nconsole.log(m.x + 1);\n",
    )
    .unwrap();
    std::fs::write(dir.join("a.cjs"), "exports.x = 41;\n").unwrap();
    let a = dump(dir, "cache-first");
    let b = dump(dir, "cache-second");
    let text = String::from_utf8_lossy(&a);
    // The test can fail: the program does compile a synthetic module, and the
    // dump names it.
    assert!(
        text.contains("<cache-dir>/synthetic-modules/perry-cjs-"),
        "the dump names no synthetic module"
    );
    if let Some(at) = text.find("cache-first") {
        let from = at.saturating_sub(120);
        panic!("the cache dir leaked: ...{}", &text[from..(at + 60).min(text.len())]);
    }
    assert!(a == b, "the dump depends on the cache dir");
}
