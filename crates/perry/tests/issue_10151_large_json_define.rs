//! Large defines must stay data, not millions of allocation/store instructions.

use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}

fn compile(root: &Path, args: &[&str]) -> String {
    // File-backed output avoids a full pipe blocking the child before timeout.
    let stdout = tempfile::tempfile().unwrap();
    let stderr = tempfile::tempfile().unwrap();
    let mut command = Command::new(
        std::env::var_os("PERRY_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_perry").into()),
    );
    command
        .current_dir(root)
        .args([
            "compile",
            "main.ts",
            "-o",
            "app.exe",
            "--no-auto-optimize",
            "--cache-dir",
            "cache",
        ])
        .args(args)
        .env_remove("PERRY_NO_CACHE")
        .env_remove("PERRY_DISABLE_BUILD_CACHE")
        .stdout(Stdio::from(stdout.try_clone().unwrap()))
        .stderr(Stdio::from(stderr.try_clone().unwrap()));
    if cfg!(windows) {
        command.env("PERRY_RS4GC", "0");
    }
    let started = Instant::now();
    let mut child = command.spawn().expect("start compiler");
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        // Normal compilation takes seconds; leave headroom for loaded CI.
        if started.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("large JSON define did not compile within 60 seconds");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    use std::io::{Read, Seek, SeekFrom};
    let read = |mut file: std::fs::File| {
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        bytes
    };
    success(Output {
        status,
        stdout: read(stdout),
        stderr: read(stderr),
    })
}

fn run(root: &Path) -> String {
    success(
        Command::new(root.join("app.exe"))
            .current_dir(root)
            .output()
            .unwrap(),
    )
}

fn config(root: &Path, value: &str) {
    std::fs::write(
        root.join("perry.json"),
        serde_json::to_vec(&serde_json::json!({"define": {"BIG": value}})).unwrap(),
    )
    .unwrap();
}

#[test]
fn megabyte_define_finishes_and_invalidates_build_and_object_caches() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("main.ts"),
        r#"
declare const BIG: Record<string, any> | undefined;
export const snapshot = typeof BIG === "undefined" ? undefined : BIG;
console.log(snapshot ? Object.keys(snapshot).length : "no snapshot");
console.log(snapshot ? snapshot.k00000.id : "missing");
"#,
    )
    .unwrap();
    // Many actual data nodes, not just a single giant string.
    let mut entries: Vec<String> = (0..12_000)
        .map(|i| {
            format!(
                r#""k{i:05}":{{"id":{i},"enabled":true,"text":"{}"}}"#,
                "x".repeat(64)
            )
        })
        .collect();
    let value = format!("{{{}}}", entries.join(","));
    assert!(value.len() > 1024 * 1024);
    config(root, &value);
    compile(root, &[]);
    assert_eq!(run(root), "12000\n0\n");
    // Reusing the same source/output/cache must observe the changed define.
    entries.push(r#""extra":null"#.into());
    config(root, &format!("{{{}}}", entries.join(",")));
    compile(root, &[]);
    assert_eq!(run(root), "12001\n0\n");
    // CLI precedence and the undefined guard still apply to a large config.
    compile(root, &["--define", "BIG=undefined"]);
    assert_eq!(run(root), "no snapshot\nmissing\n");
}

#[test]
fn small_define_stays_direct_and_large_reads_create_fresh_values() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("main.ts"),
        r#"
function read(JSON: any) { return BIG; }
const a = read(null);
const b = read(null);
console.log(Object.keys(a).join(","));
console.log(a.items[0], a.items[1], a.items[2], a.name);
console.log(a === b);
a.items[0] = 9;
console.log(b.items[0]);
"#,
    )
    .unwrap();
    let value = r#"{items:[1,true,null],name:"small"}"#;
    let define = format!("BIG={value}");
    let hir = compile(root, &["--define", &define, "--print-hir"]);
    assert!(hir.contains("=== HIR"), "must inspect an actual HIR dump");
    assert!(
        !hir.contains("JsonParse("),
        "small literals must lower directly"
    );
    assert_eq!(run(root), "items,name\n1 true null small\nfalse\n1\n");

    // A record above the primitive-array text threshold still stays direct.
    let mid = format!(
        r#"{{items:[1,true,null],name:"small",padding:"{}"}}"#,
        "x".repeat(65_536)
    );
    config(root, &mid);
    let hir = compile(root, &["--print-hir"]);
    assert!(hir.contains("=== HIR") && !hir.contains("JsonParse("));
    assert_eq!(
        run(root),
        "items,name,padding\n1 true null small\nfalse\n1\n"
    );

    // Large define literals share the lowering path. Shadowing JSON must not
    // intercept compiler-generated parsing, and mutations must stay local.
    let large = format!(
        r#"{{items:[1,true,null],name:"small",padding:"{}"}}"#,
        "x".repeat(1024 * 1024)
    );
    config(root, &large);
    let hir = compile(root, &["--print-hir"]);
    assert!(hir.contains("JsonParse("), "huge records must stay compact");
    assert_eq!(
        run(root),
        "items,name,padding\n1 true null small\nfalse\n1\n"
    );
}

#[test]
fn large_array_define_preserves_array_behavior_and_numeric_values() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut items = vec!["0"; 1024];
    items[0] = "-0";
    items[1] = "0xff";
    items[2] = "+2";
    config(root, &format!("[{}]", items.join(",")));
    std::fs::write(
        root.join("main.ts"),
        r#"
const a = BIG;
console.log(Array.isArray(a), a.length, Object.is(a[0], -0), a[1], a[2]);
a.push(7);
console.log(a.length, a.pop(), a.length);
"#,
    )
    .unwrap();
    let hir = compile(root, &["--print-hir"]);
    assert!(
        hir.contains("JsonParse("),
        "primitive arrays keep the fast path"
    );
    assert_eq!(run(root), "true 1024 true 255 2\n1025 7 1024\n");
}

#[test]
fn source_literal_preserves_key_order_and_duplicate_last_write() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = format!(
        r#"
const value = {{z:0,a:-0,z:+2,1e-7:0xff,nested:[true,null,"quote\"\n\\☃"],padding:"{}"}};
console.log(Object.keys(value).join(","));
console.log(value.z, Object.is(value.a, -0), value[1e-7]);
console.log(JSON.stringify(value.nested));
"#,
        "x".repeat(1024 * 1024),
    );
    std::fs::write(root.join("main.ts"), source).unwrap();
    compile(root, &[]);
    assert_eq!(
        run(root),
        "z,a,1e-7,nested,padding\n2 true 255\n[true,null,\"quote\\\"\\n\\\\☃\"]\n"
    );
}
