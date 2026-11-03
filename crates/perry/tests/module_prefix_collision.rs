//! Two modules whose names sanitize to the same symbol prefix
//! (`providers/google-vertex-chat.ts` and `providers/google-vertex/chat.ts`
//! both become `providers_google_vertex_chat_ts`) must keep distinct
//! symbols. Before the fix their `perry_fn_<prefix>__*` symbols merged: the
//! fixture below failed at run time with "value is not a function", and
//! OpenCode v2.1.2 failed to link (undefined `perry_fn_..._google_vertex_chat_ts__model`).
//!
//! `node main.ts` and `bun main.ts` print exactly the asserted output.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn modules_with_colliding_sanitized_names_keep_distinct_symbols() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "module-prefix-collision", "private": true, "type": "module" }"#,
    )
    .expect("write package.json");
    let providers = root.join("providers");
    std::fs::create_dir_all(providers.join("google-vertex")).expect("mkdir providers");
    std::fs::write(
        providers.join("google-vertex-chat.ts"),
        r#"export const id = "vertex-chat"
export const configure = (suffix = "") => ({ id, model: (modelID: string) => `chat:${modelID}${suffix}` })
export const model = (modelID: string, { suffix }: { suffix?: string } = {}) => configure(suffix).model(modelID)
export function describe(): string {
  return `module ${id}`
}
"#,
    )
    .expect("write google-vertex-chat.ts");
    std::fs::write(
        providers.join("google-vertex").join("chat.ts"),
        r#"export { model } from "../google-vertex-chat.ts"
export const id = "nested-chat"
export function describe(): string {
  return `module ${id}`
}
"#,
    )
    .expect("write google-vertex/chat.ts");
    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"import * as Flat from "./providers/google-vertex-chat.ts"
import * as Nested from "./providers/google-vertex/chat.ts"
const dynamicNested = await import("./providers/google-vertex/chat.ts")
console.log(Flat.id, Flat.describe(), Flat.model("a", { suffix: "!" }))
console.log(Nested.id, Nested.describe(), Nested.model("b"))
console.log(dynamicNested.model === Flat.model, Object.keys(Nested).sort().join(","))
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
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "vertex-chat module vertex-chat chat:a!\n\
         nested-chat module nested-chat chat:b\n\
         true describe,id,model\n"
    );
}
