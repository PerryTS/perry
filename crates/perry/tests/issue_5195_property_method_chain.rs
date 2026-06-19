//! Regression tests for #5195 — an inline property-access chained into an
//! array/string-overlapping method call (`arr[i].id.indexOf(x)`,
//! `p.id.includes(x)`) miscompiled and returned the wrong result.
//!
//! Root cause: `lower/expr_call/array_only_methods.rs` folded a `PropertyGet`
//! receiver of `.indexOf` / `.includes` to `ArrayIndexOf` / `ArrayIncludes`
//! (an element-equality search) unless the property name happened to be in a
//! hardcoded `stack|message|name|sourceSQL|expandedSQL` allowlist. A genuine
//! string property (`id: string`) fell through that allowlist and was searched
//! as an array, so `arr[i].id.indexOf("PRO")` returned -1 (and the equivalent
//! `.filter(p => p.id.indexOf("PRO") >= 0)` predicate returned 0 matches) with
//! no error — silently breaking a StoreKit product lookup downstream.
//!
//! The fix only folds receivers that are *provably* array-producing
//! (`.map`/`.filter`/`.split`/`Object.keys`/array literals/…). Every property
//! access now falls through to `js_native_call_method`, which checks the
//! runtime value type and routes string- vs. array-`indexOf` correctly — the
//! same way the `slice` arm already behaved.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, entry: &std::path::Path) -> String {
    let output = dir.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(entry)
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
    String::from_utf8_lossy(&run.stdout).to_string()
}

/// The exact issue shape: a string property read inline and immediately
/// `.indexOf`/`.includes`'d, both in a loop and inside a `.filter` predicate.
/// Pre-fix every count was 0; they must match the extract-to-local form.
#[test]
fn string_property_indexof_includes_inline_match() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
interface P { id: string }
const arr: P[] = [{ id: "GSC_PRO_MONTHLY" }, { id: "GSC_AGENCY_MONTHLY" }, { id: "GSC_PRO_ANNUAL" }]

let inlineIdx = 0
for (let i = 0; i < arr.length; i++) { if (arr[i].id.indexOf("PRO") >= 0) inlineIdx++ }
console.log("inlineIdx=" + inlineIdx)

let inlineInc = 0
for (let i = 0; i < arr.length; i++) { if (arr[i].id.includes("PRO")) inlineInc++ }
console.log("inlineInc=" + inlineInc)

console.log("filter=" + arr.filter(function (p: P) { return p.id.indexOf("PRO") >= 0 }).length)
console.log("chain=" + arr[0].id.toLowerCase().indexOf("pro"))
"#,
    )
    .expect("write entry");

    let stdout = compile_and_run(dir.path(), &entry);
    assert!(
        stdout.contains("inlineIdx=2"),
        "arr[i].id.indexOf(\"PRO\") must find 2 (got: {stdout})"
    );
    assert!(
        stdout.contains("inlineInc=2"),
        "arr[i].id.includes(\"PRO\") must find 2 (got: {stdout})"
    );
    assert!(
        stdout.contains("filter=2"),
        "filter(p => p.id.indexOf(\"PRO\") >= 0) must keep 2 (got: {stdout})"
    );
    assert!(
        stdout.contains("chain=4"),
        "arr[0].id.toLowerCase().indexOf(\"pro\") must be 4 (got: {stdout})"
    );
}

/// The fix must NOT regress a genuinely array-typed property: `.indexOf` /
/// `.includes` on an array field still search the array (via the runtime-typed
/// generic dispatch) and return the element index / membership.
#[test]
fn array_typed_property_indexof_includes_still_search_array() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
class Box { tags: string[] = ["red", "green", "blue"] }
const b = new Box()
console.log("idx=" + b.tags.indexOf("green"))
console.log("inc=" + b.tags.includes("blue"))
console.log("miss=" + b.tags.indexOf("pink"))
const cfg = { nums: [10, 20, 30] }
console.log("nidx=" + cfg.nums.indexOf(20))
"#,
    )
    .expect("write entry");

    let stdout = compile_and_run(dir.path(), &entry);
    assert!(
        stdout.contains("idx=1"),
        "array-property indexOf must return the element index (got: {stdout})"
    );
    assert!(
        stdout.contains("inc=true"),
        "array-property includes must return true for a member (got: {stdout})"
    );
    assert!(
        stdout.contains("miss=-1"),
        "array-property indexOf must return -1 for a non-member (got: {stdout})"
    );
    assert!(
        stdout.contains("nidx=1"),
        "numeric array-property indexOf must return the index (got: {stdout})"
    );
}
