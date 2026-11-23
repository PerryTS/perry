//! #10504: `%Object.prototype%` built on its first use by a program operation
//! (`Object.getPrototypeOf`, `Object.setPrototypeOf`, a function's
//! `.prototype`) is the runtime's own definition. Its member installs must
//! not set the shadow bit (bit 30 of the layout word) when the program names
//! one of its members (here `__proto__`), or `Pristine` is withdrawn from
//! every builtin family and each checked builtin entry (`s.charCodeAt(i)`, …)
//! takes the generic path for the rest of the process.

use std::path::PathBuf;
use std::process::Command;

const SHADOW_BIT: u32 = 1 << 30;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// The layout word `PERRY_BUILTIN_PROTO_TRACE` reports for each seal of
/// `%Object.prototype%` while `program` runs.
fn object_prototype_seal_words(program: &str) -> Vec<u32> {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(&entry, program).expect("write program");
    let output = dir.path().join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-auto-optimize")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&output)
        .env("PERRY_BUILTIN_PROTO_TRACE", "1")
        .output()
        .expect("run compiled binary");
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        run.status.success(),
        "compiled binary failed\nstdout:\n{}\nstderr:\n{stderr}",
        String::from_utf8_lossy(&run.stdout),
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "98 true\n");
    stderr
        .lines()
        .filter_map(|line| line.strip_prefix("[builtin-proto] sealed Object word=0x"))
        .map(|hex| u32::from_str_radix(hex.trim(), 16).expect("hex layout word"))
        .collect()
}

#[test]
fn object_prototype_built_by_a_program_operation_stays_pristine() {
    for first_use in [
        "Object.getPrototypeOf({ a: 1 })",
        "Object.setPrototypeOf({ a: 1 }, { b: 2 })",
        "(function Base() {} as any).prototype",
    ] {
        let program = format!(
            "let sink: any = {first_use};\n\
             class Holder {{\n\
               text: string;\n\
               constructor(text: string) {{ this.text = text; }}\n\
               at(i: number) {{ return this.text.charCodeAt(i); }}\n\
             }}\n\
             const o: any = {{ a: 1 }};\n\
             console.log(new Holder(typeof sink).at(1), o.__proto__ === Object.prototype);\n"
        );
        let words = object_prototype_seal_words(&program);
        assert!(
            !words.is_empty(),
            "{first_use}: no %Object.prototype% seal was traced"
        );
        for word in words {
            assert_eq!(
                word & SHADOW_BIT,
                0,
                "{first_use}: %Object.prototype% sealed with the shadow bit (word {word:#x})"
            );
        }
    }
}
