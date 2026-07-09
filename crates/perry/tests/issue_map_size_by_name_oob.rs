//! Regression: reading `.size` on a `Map` whose static type has been erased to
//! `any` — as a minified/bundled program produces when a `Map` flows through a
//! dynamically-dispatched `getX(): any` call — dispatched the read *by name*
//! through `js_object_get_field_by_name`. Its `class X extends Map|Set` `.size`
//! fast path calls `own_key_present(map, "size")`, which read `(*obj).keys_array`
//! at `ObjectHeader` offset 16. A `MapHeader` is only 16 bytes
//! (`size`/`capacity`/`entries`) with no `keys_array` field, so that load went
//! out of bounds into the adjacent allocation; when the stray word was a live
//! neighbour's GC-header value it cleared the keys-pointer guard and then
//! SIGBUS'd on the `[keys-8]` GC-type-tag read.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
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

    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary crashed (Bug: OOB keys_array read on a Map receiver)\n\
         status: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn any_typed_map_size_by_name_does_not_crash() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function makeManager(): any {
  const q = new Map<string, { state: string }>();
  const b = new Map<string, number>();
  const c = new Map<string, number>();
  q.set("s1", { state: "running" });
  q.set("s2", { state: "error" });
  function getAllServers() { return q; }
  function count() { return b.size + c.size; }
  return { getAllServers, count };
}
let mgr: any;
function getManager(): any { return mgr; }
function anyEnabled(): boolean {
  const m = getManager();
  if (!m) return false;
  const servers = m.getAllServers(); // `any`: the Map's static type is erased
  if (servers.size === 0) return false; // by-name `.size` read on a Map
  for (const s of servers.values()) if (s.state !== "error") return true;
  return false;
}
console.log("first", anyEnabled());
mgr = makeManager();
console.log("size", getManager().getAllServers().size);
console.log("second", anyEnabled());
console.log("done");
"#,
    );
    assert!(stdout.contains("first false"), "first: {stdout}");
    assert!(stdout.contains("size 2"), "map .size by name: {stdout}");
    assert!(stdout.contains("second true"), "second: {stdout}");
    assert!(stdout.contains("done"), "reached end: {stdout}");
}
