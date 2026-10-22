//! A namespace import's member must not change the arity of a same-named
//! named import. effect's `Schema.ts` imports `{ format }` from
//! `Formatter.ts` (2 parameters) and `* as Duration_` (whose `format` takes
//! 1); the member's flat-map entry overwrote the arity, so `format(source)`
//! was emitted without the `undefined` padding for the second parameter and
//! the callee read a stale register (OpenCode v2.1.2 segfaulted at startup in
//! `Formatter.format`). Here the stale register shows as a wrong default.
//!
//! `node main.ts` and `bun main.ts` print exactly the asserted line.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn namespace_member_does_not_override_named_import_arity() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "namespace-member-arity", "private": true, "type": "module" }"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("formatter.ts"),
        r#"export function format(input: unknown, space: number = 7): string {
  if (Array.isArray(input) && input.length > 0) return format(input[0], space) + "+"
  return JSON.stringify(input) + "/" + String(space)
}
export function pair(a: number, b: number): number {
  return a * 10 + b
}
"#,
    )
    .expect("write formatter.ts");
    std::fs::write(
        root.join("duration.ts"),
        r#"export const format = (self: { n: number }): string => {
  const parts = ["D", String(self.n)]
  return parts.join("")
}
"#,
    )
    .expect("write duration.ts");
    std::fs::write(
        root.join("schema.ts"),
        r#"import { format, pair } from "./formatter.ts"
import * as Duration from "./duration.ts"
export function isPattern(regExp: RegExp): string {
  const p = pair(regExp.source.length, 2.5)
  return `${format(regExp.source)} ${Duration.format({ n: p })}`
}
"#,
    )
    .expect("write schema.ts");
    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"import { isPattern } from "./schema.ts"
console.log(isPattern(/^a+$/u), isPattern(/b/))
"#,
    )
    .expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(run.status.success(), "status: {:?}", run.status);
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "\"^a+$\"/7 D42.5 \"b\"/7 D12.5\n"
    );
}
