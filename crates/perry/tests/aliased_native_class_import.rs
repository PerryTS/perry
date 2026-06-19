//! Regression test: an ALIASED ESM named import of a native built-in class
//! must keep the native-class identity, exactly like the un-aliased form.
//!
//!   import { BlockList as Wj4 } from "net";
//!   const q = new Wj4();
//!   q.addSubnet(...)            // pre-fix: "addSubnet is not a function"
//!
//! Root cause: native-class resolution keyed on the LOCAL binding name. Under
//! an alias (`Wj4`) the local name missed every literal-name arm, so:
//!   (a) construction (`expr_new.rs`) fell through to `Expr::New { class_name:
//!       "Wj4" }`, which codegen's builtin-`New` dispatch did not recognize, so
//!       it built an empty placeholder object with no native methods; and
//!   (b) the native-instance tag (`destructuring/var_decl.rs`) registered the
//!       binding under `("net","Wj4")`, so `q.<method>()` missed the
//!       `("net","BlockList")` dispatch rows.
//!
//! Fix: resolve the local alias to its IMPORTED export name via the
//! (alias-aware) native-module import map at both sites, so the aliased path is
//! byte-for-byte identical to the un-aliased path. Covers every aliased native
//! class (net `Socket`/`BlockList`, http `Server`, url `URL`, …), not a
//! per-class literal-name patch. A non-native user import alias must NOT be
//! treated as a native class (no over-trigger).

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &Path, source: &str) -> String {
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
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// Print the HIR for `source` (the `--print-hir` debug dump).
fn print_hir(dir: &Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
    let out = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("--print-hir")
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile --print-hir");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Aliased `net.BlockList`: the aliased construction + method dispatch must
/// lower to the SAME canonical native-class HIR as the un-aliased form
/// (`New { class_name: "BlockList" }` and `NativeMethodCall { class_name:
/// Some("BlockList") }`). Asserted at the HIR layer because BlockList's runtime
/// construction has a separate, pre-existing "Invalid socket address" bug that
/// affects the un-aliased path identically and is out of scope for this fix —
/// the HIR-identity check isolates the alias-resolution behavior under test.
#[test]
fn aliased_net_block_list_lowers_like_unaliased() {
    let aliased = {
        let dir = tempfile::tempdir().expect("tempdir");
        print_hir(
            dir.path(),
            r#"
import { BlockList as Wj4 } from "net";
const q: any = new Wj4();
console.log(typeof q.addSubnet, typeof q.check);
"#,
        )
    };
    let unaliased = {
        let dir = tempfile::tempdir().expect("tempdir");
        print_hir(
            dir.path(),
            r#"
import { BlockList } from "net";
const q: any = new BlockList();
console.log(typeof q.addSubnet, typeof q.check);
"#,
        )
    };
    // The aliased form must resolve to the canonical "BlockList" class at both
    // the construction site and the method-dispatch tag.
    assert!(
        aliased.contains("New { class_name: \"BlockList\""),
        "aliased BlockList construction did not resolve to the canonical \
         class name; HIR:\n{aliased}"
    );
    assert!(
        aliased.contains("class_name: Some(\"BlockList\")"),
        "aliased BlockList method dispatch was not tagged ( net , BlockList ); \
         HIR:\n{aliased}"
    );
    // It must NOT carry the local alias name anywhere a native class is named.
    assert!(
        !aliased.contains("class_name: \"Wj4\"") && !aliased.contains("class_name: Some(\"Wj4\")"),
        "aliased BlockList still carries the local alias name; HIR:\n{aliased}"
    );
    // And it must match the un-aliased HIR for the relevant lines, modulo
    // `byte_offset` (the alias source text is longer, so offsets legitimately
    // shift — only the class identity must be identical).
    let pick = |hir: &str| -> Vec<String> {
        hir.lines()
            .filter(|l| l.contains("BlockList") || l.contains("addSubnet"))
            .map(|l| {
                // Drop every `byte_offset: <n>` so position-only differences
                // don't fail the identity check.
                let mut out = String::new();
                let mut rest = l.trim();
                while let Some(idx) = rest.find("byte_offset: ") {
                    out.push_str(&rest[..idx]);
                    rest = &rest[idx + "byte_offset: ".len()..];
                    rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
                }
                out.push_str(rest);
                out
            })
            .collect()
    };
    assert_eq!(pick(&aliased), pick(&unaliased));
}

/// Aliased `url.URL`: full construct + property read must equal node.
#[test]
fn aliased_url_constructs_and_reads_properties() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { URL as U } from "url";
const u = new U("http://example.com/path?q=1");
console.log("host:", u.host);
console.log("pathname:", u.pathname);
console.log("search:", u.search);
"#,
    );
    assert_eq!(stdout, "host: example.com\npathname: /path\nsearch: ?q=1\n");
}

/// Aliased `net.Socket`: native methods must resolve.
#[test]
fn aliased_net_socket_keeps_native_methods() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { Socket as Sk } from "net";
const s: any = new Sk();
console.log("connect:", typeof s.connect);
console.log("write:", typeof s.write);
console.log("destroy:", typeof s.destroy);
"#,
    );
    assert_eq!(
        stdout,
        "connect: function\nwrite: function\ndestroy: function\n"
    );
}

/// The aliased binding must resolve to the SAME native methods the un-aliased
/// binding does (alias path == canonical path).
#[test]
fn aliased_matches_unaliased_for_native_class() {
    let dir = tempfile::tempdir().expect("tempdir");
    let aliased = compile_and_run(
        dir.path(),
        r#"
import { Socket as Sk } from "net";
const s: any = new Sk();
console.log(typeof s.connect, typeof s.write, typeof s.end);
"#,
    );
    let dir2 = tempfile::tempdir().expect("tempdir");
    let unaliased = compile_and_run(
        dir2.path(),
        r#"
import { Socket } from "net";
const s: any = new Socket();
console.log(typeof s.connect, typeof s.write, typeof s.end);
"#,
    );
    assert_eq!(aliased, unaliased);
    assert_eq!(aliased, "function function function\n");
}

/// A non-native user import alias must NOT be treated as a native class — the
/// fix must not over-trigger native handling on ordinary user modules.
#[test]
fn aliased_user_class_import_is_not_treated_as_native() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("dep.ts"),
        "export class Foo { greet() { return \"hi from Foo\"; } }\n\
         export function bar() { return 42; }\n",
    )
    .expect("write dep");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { Foo as Baz, bar as qux } from "./dep";
const f = new Baz();
console.log("greet:", f.greet());
console.log("bar:", qux());
"#,
    );
    assert_eq!(stdout, "greet: hi from Foo\nbar: 42\n");
}
