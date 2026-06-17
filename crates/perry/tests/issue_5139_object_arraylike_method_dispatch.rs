//! Regression tests for #5139 — `react-dom/server`'s `renderToStaticMarkup`
//! returned an empty string.
//!
//! Root cause was in HIR lowering, not the SSR library: a method call whose
//! name collides with an `Array.prototype` mutator (`push`/`pop`/`shift`/
//! `unshift`/`splice`/`sort`/`reverse`/`concat`) on a receiver whose static
//! type is `any` was eagerly lowered to the array-only fast path
//! (`Expr::ArrayPush` / the `array.push_single` native arm). That reads the
//! receiver's header as an `ArrayHeader`, so a *plain object* that merely owns
//! a same-named closure property — react-dom's Fizz `destination = { push(chunk)
//! { result += chunk } }`, passed through several `any`-typed params before
//! `writeChunk(destination, …)` calls `destination.push` — had its bytes read
//! as array length/capacity. `push` returned a bogus numeric length and the
//! user closure never ran, so every chunk was dropped and the SSR result stayed
//! empty.
//!
//! The fix defers these mutator names on unknown/`any` receivers to the runtime
//! `js_native_call_method` dispatch, which selects by the receiver's *runtime*
//! shape: a real array hits the dense `js_array_*` helpers (growth still
//! resolves through the #233 forwarding pointer), while a plain object with an
//! own callable of that name invokes it with `this` bound to the receiver.

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

/// The reduced shape of react-dom's Fizz flush loop: a plain-object sink with a
/// `push` closure, handed through `any`-typed params before being invoked. The
/// closure must run (mutating its captured `result`), not be replaced by the
/// array builtin (which would return a length and drop the write).
#[test]
fn object_push_method_through_any_param_runs_user_closure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
let result = "";
const destination = { push: function (chunk: any) { if (chunk !== null) result += chunk; return true; } };

function writeChunkAndReturn(d: any, chunk: any) { return d.push(chunk); }
function writeChunk(d: any, chunk: any) { writeChunkAndReturn(d, chunk); }

function flushSubtree(d: any, segment: any) {
  const chunks = segment.chunks;
  let chunkIdx = 0;
  for (; chunkIdx < chunks.length - 1; chunkIdx++) {
    writeChunk(d, chunks[chunkIdx]);
  }
  if (chunkIdx < chunks.length) {
    writeChunkAndReturn(d, chunks[chunkIdx]);
  }
}

flushSubtree(destination, { chunks: ["<ul", ">", "x", "</ul", ">"] });
console.log("result=[" + result + "]");
"#,
    )
    .expect("write entry");

    let stdout = compile_and_run(dir.path(), &entry);
    assert!(
        stdout.contains("result=[<ul>x</ul>]"),
        "object's own push closure must run when invoked through any-typed params, \
         not the Array.prototype.push builtin (got: {stdout})"
    );
}

/// All the array-mutator names a plain object can legitimately own as a closure
/// property must dispatch to the user method, with `this` bound to the receiver
/// and captured-variable mutations visible.
#[test]
fn object_arraylike_mutator_names_dispatch_to_own_methods() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
let log = "";
const o = {
  push: function () { log += "P"; },
  pop: function () { log += "p"; },
  shift: function () { log += "s"; },
  unshift: function () { log += "u"; },
  splice: function () { log += "S"; },
  sort: function () { log += "o"; },
  reverse: function () { log += "r"; },
  concat: function () { log += "c"; },
};
function call(x: any) {
  x.push(); x.pop(); x.shift(); x.unshift(); x.splice(); x.sort(); x.reverse(); x.concat();
}
call(o);
console.log("log=[" + log + "]");
"#,
    )
    .expect("write entry");

    let stdout = compile_and_run(dir.path(), &entry);
    assert!(
        stdout.contains("log=[PpsuSorc]"),
        "every Array-mutator-named own method must dispatch to the object's closure (got: {stdout})"
    );
}

/// The fix must NOT regress real arrays held in `any`-typed values: mutators
/// still behave like `Array.prototype`, including growth that reallocates
/// across a function boundary (resolved via the #233 forwarding pointer).
#[test]
fn any_typed_real_arrays_keep_array_mutator_semantics() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
function pushAll(a: any, n: number) { for (let i = 0; i < n; i++) a.push(i); return a.length; }
const arr: any = [];
const len = pushAll(arr, 1000); // forces several reallocs across the call boundary
let sum = 0; for (let i = 0; i < arr.length; i++) sum += arr[i];
console.log("grow:", len, arr.length, sum);

function ops(a: any) { a.unshift(0); a.push(99); a.reverse(); return a.splice(1, 1)[0]; }
const b: any = [2, 3];
const removed = ops(b);
console.log("ops:", JSON.stringify(b), removed);

function sortDesc(a: any) { a.sort((x: number, y: number) => y - x); return a; }
console.log("sort:", JSON.stringify(sortDesc([3, 1, 2] as any)));

const c: any = [10, 20, 30];
console.log("popshift:", c.pop !== undefined ? "ok" : "no");
function ps(a: any) { return [a.pop(), a.shift()]; }
console.log("popshift2:", JSON.stringify(ps(c)), JSON.stringify(c));
"#,
    )
    .expect("write entry");

    let stdout = compile_and_run(dir.path(), &entry);
    assert!(
        stdout.contains("grow: 1000 1000 499500"),
        "any-typed array push must grow correctly across a call boundary (got: {stdout})"
    );
    // b = [2,3] -> unshift 0 -> [0,2,3] -> push 99 -> [0,2,3,99] -> reverse -> [99,3,2,0]
    //   -> splice(1,1) removes the 3 -> b = [99,2,0], removed = 3
    assert!(
        stdout.contains("ops: [99,2,0] 3"),
        "any-typed array unshift/push/reverse/splice must match Array semantics (got: {stdout})"
    );
    assert!(
        stdout.contains("sort: [3,2,1]"),
        "any-typed array sort with comparator must work (got: {stdout})"
    );
    assert!(
        stdout.contains("popshift2: [30,10] [20]"),
        "any-typed array pop/shift must mutate in place (got: {stdout})"
    );
}
