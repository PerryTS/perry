//! A typed array whose tracked buffer view was demoted (it escaped, so the
//! view lost its `noalias` proof and alias scope) reads its elements through
//! the per-site guarded inline load, as an untracked receiver does.
//!
//! No view tier serves a demoted view: `lower_typed_array_load` and the proven
//! checked and guarded tiers all require the proof. The guarded load used to
//! refuse any receiver with a tracked view, so a demoted view fell to the
//! unconditional `js_typed_array_get` call plus a `js_number_coerce`: worse
//! than having no view at all. One exception stays on that call: a view whose
//! `.buffer` was exposed. Its elements move out of line, every guarded read
//! would miss the kind cache, and the helper behind the miss costs more than
//! the plain call.
//!
//! The behavioural half (in-bounds, out-of-bounds, callee writes, `.buffer`,
//! detach and reassignment, against node) is
//! `test-files/test_gap_downgraded_view_guarded_read.ts`.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const SOURCE: &str = r#"
let sink: any = null;
function keep(x: any) { sink = x; }
const dyn: any = keep;
export function escaped(n: number): number {
  const a = new Float64Array(16);
  for (let i = 0; i < 16; i++) a[i] = i * 0.5;
  dyn(a);
  let s = 0;
  for (let r = 0; r < n; r++) for (let i = 0; i < 16; i++) s += a[i];
  return s;
}
export function exposed(n: number): number {
  const a = new Float64Array(16);
  for (let i = 0; i < 16; i++) a[i] = i * 0.5;
  keep(a.buffer);
  let s = 0;
  for (let r = 0; r < n; r++) for (let i = 0; i < 16; i++) s += a[i];
  return s;
}
console.log(escaped(3), exposed(3));
"#;

/// The body of the first `define` whose symbol contains `needle`.
fn function_body<'a>(ir: &'a str, needle: &str) -> Option<&'a str> {
    let mut offset = 0;
    for line in ir.split_inclusive('\n') {
        if line.starts_with("define ") && line.contains(needle) {
            let rest = &ir[offset..];
            let end = rest.find("\n}\n").map(|i| i + 3).unwrap_or(rest.len());
            return Some(&rest[..end]);
        }
        offset += line.len();
    }
    None
}

fn compile_ir() -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("dvread.ts");
    std::fs::write(&entry, SOURCE).expect("write entry");
    let ll = dir.path().join("ll");
    std::fs::create_dir_all(&ll).expect("ll dir");
    let out = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(dir.path().join("dvread"))
        .arg("--no-cache")
        .arg("--no-link")
        .env("PERRY_SAVE_LL", &ll)
        .output()
        .expect("run perry compile");
    assert!(
        out.status.success(),
        "compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    std::fs::read_dir(&ll)
        .expect("read ll dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "ll"))
        .map(|e| std::fs::read_to_string(e.path()).expect("read .ll"))
        .collect()
}

#[test]
fn a_demoted_view_reads_through_the_guarded_inline_load() {
    let ir = compile_ir();
    let escaped = function_body(&ir, "dvread_ts__escaped")
        .unwrap_or_else(|| panic!("no escaped() in:\n{ir}"));
    // Witness: the array was allocated in the function (so it had a view) and
    // escaped through the dynamic call that demotes it.
    assert!(
        escaped.contains("@js_typed_array_new_empty("),
        "escaped(): the typed array is no longer allocated here:\n{escaped}"
    );
    assert!(
        escaped.contains("ctaf.get.load"),
        "escaped(): the demoted view's reads did not take the guarded load:\n{escaped}"
    );
    assert!(
        !escaped.contains("@js_typed_array_get("),
        "escaped(): a read still fell to js_typed_array_get:\n{escaped}"
    );
}

#[test]
fn an_exposed_buffer_view_keeps_the_plain_call() {
    let ir = compile_ir();
    let exposed = function_body(&ir, "dvread_ts__exposed")
        .unwrap_or_else(|| panic!("no exposed() in:\n{ir}"));
    assert!(
        exposed.contains("@js_typed_array_get("),
        "exposed(): witness lost, the read no longer takes the plain call:\n{exposed}"
    );
    assert!(
        !exposed.contains("ctaf.get.load"),
        "exposed(): an out-of-line view took the guarded load, which always misses:\n{exposed}"
    );
}
