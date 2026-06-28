//! Regression coverage for TanStack Start SSR streaming through Response
//! wrappers. The app path constructs `Response(ReadableStream)` values whose
//! chunks are produced lazily from downstream pulls; eagerly draining only
//! already-buffered chunks turns a valid HTML response into an empty body.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.js");
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
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

fn compile_and_run_entry(dir: &std::path::Path, entry_name: &str) -> String {
    let entry = dir.join(entry_name);
    let output = dir.join("main_bin");

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
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn response_preserves_pull_driven_readable_stream_body() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const enc = new TextEncoder()
let pulls = 0
const stream = new ReadableStream({
  pull(controller) {
    pulls++
    controller.enqueue(enc.encode('hello'))
    controller.close()
  }
})
const response = new Response(stream, { status: 200 })
const reader = response.body.getReader()
const first = await reader.read()
console.log('done=' + first.done + ',len=' + (first.value ? first.value.byteLength : 0) + ',pulls=' + pulls)
"#,
    );
    assert_eq!(stdout, "done=false,len=5,pulls=1\n");
}

#[test]
fn response_prototype_exposes_fetch_accessors_for_wrappers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const names = Object.getOwnPropertyNames(Response.prototype)
const body = Object.getOwnPropertyDescriptor(Response.prototype, 'body')
const headers = Object.getOwnPropertyDescriptor(Response.prototype, 'headers')
console.log(names.includes('body') + ',' + (typeof body?.get) + ',' + names.includes('headers') + ',' + (typeof headers?.get))
"#,
    );
    assert_eq!(stdout, "true,function,true,function\n");
}

#[test]
fn dynamic_import_inside_arrow_closure_is_collected() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("main.js"),
        r#"
const importer = () => import('./lazy.js')
const mod = await importer()
console.log('answer=' + mod.answer)
"#,
    )
    .expect("write main");
    std::fs::write(dir.path().join("lazy.js"), "export const answer = 42\n").expect("write lazy");
    let stdout = compile_and_run_entry(dir.path(), "main.js");
    assert_eq!(stdout, "answer=42\n");
}

#[test]
fn script_string_query_imports_compile_to_default_string_asset() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{
          "type": "module",
          "perry": {
            "compilePackages": ["pkg"],
            "allow": { "compilePackages": ["pkg"] }
          }
        }"#,
    )
    .expect("write package");
    let pkg = dir.path().join("node_modules/pkg");
    std::fs::create_dir_all(pkg.join("src")).expect("mkdir pkg");
    std::fs::write(
        pkg.join("package.json"),
        r#"{
          "name": "pkg",
          "type": "module",
          "exports": { ".": { "import": { "default": "./src/index.ts" } } }
        }"#,
    )
    .expect("write pkg package");
    std::fs::write(
        pkg.join("src/index.ts"),
        "import boot from './boot?script-string'\nexport function readBoot() { return boot }\n",
    )
    .expect("write pkg index");
    std::fs::write(pkg.join("src/boot.ts"), "self.$_TSR = { buffer: [] }\n")
        .expect("write script source");
    std::fs::write(
        dir.path().join("main.js"),
        "import { readBoot } from 'pkg'\nconst boot = readBoot()\nconsole.log(typeof boot)\nconsole.log(boot.includes('self.$_TSR ='))\nconsole.log(boot === true)\n",
    )
    .expect("write main");

    let stdout = compile_and_run_entry(dir.path(), "main.js");
    assert_eq!(stdout, "string\ntrue\nfalse\n");
}

#[test]
fn map_foreach_property_receiver_preserves_map_callback_shape() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const renderState = { styles: new Map() }
renderState.styles.set('default', {
  precedence: 'default',
  sheets: new Map([['/assets/styles.css', { href: '/assets/styles.css' }]])
})
const seen = []
renderState.styles.forEach(function(styleQueue, key, map) {
  seen.push(
    key + ':' +
    styleQueue.precedence + ':' +
    styleQueue.sheets.size + ':' +
    (map === renderState.styles) + ':' +
    this.destination
  )
}, { destination: 'html' })
console.log(seen.join('|'))
"#,
    );
    assert_eq!(stdout, "default:default:1:true:html\n");
}
