//! Regression test for #5139: an object with a user-defined `push` method,
//! invoked through an `any`-typed parameter, must dispatch to that method —
//! not be silently read as an `ArrayHeader` by the `array.push_single`
//! intrinsic. The HIR lowering eagerly assumed any `x.push(v)` on an unknown
//! receiver was an array push; for react-dom's `renderToStaticMarkup` the
//! output sink is `{ push(chunk){ result += chunk } }` threaded through
//! `writeChunk(destination, c) { return destination.push(c); }`, so the closure
//! never ran and SSR markup came back as the empty string.
//!
//! The shape below is the distilled core of that bug. We also exercise a real
//! `any`-typed array through the same parameter-call path to prove the fix
//! routes real arrays to the runtime dispatch correctly (no regression).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn object_push_method_dispatches_through_any_param() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");

    std::fs::write(
        &entry,
        r#"
// An object whose `push` mutates a captured local — the react-dom SSR sink shape.
function writeChunk(d, c) { return d.push(c); }
function render() {
  var result = '';
  var destination = {
    push: function (chunk) { if (chunk !== null) { result += chunk; } return true; },
    destroy: function (error) {}
  };
  writeChunk(destination, '<ul>');
  writeChunk(destination, '<li>a</li>');
  writeChunk(destination, '</ul>');
  return result;
}
console.log(render());

// The whole stack/queue/deque family must dispatch to user object methods,
// not the array intrinsics, when reached through an any-typed parameter.
function callPush(d, v) { return d.push(v); }
function callPop(d) { return d.pop(); }
function callShift(d) { return d.shift(); }
function callUnshift(d, v) { return d.unshift(v); }
function deque() {
  var trace = '';
  var obj = {
    push: function (v) { trace += 'PU' + v; return 1; },
    pop: function () { trace += 'PO'; return 2; },
    shift: function () { trace += 'SH'; return 3; },
    unshift: function (v) { trace += 'UN' + v; return 4; },
  };
  callPush(obj, 'a'); callPop(obj); callShift(obj); callUnshift(obj, 'b');
  return trace;
}
console.log(deque());

// A real array reached through the same any-typed parameter path: mutators
// must still behave like Array.prototype.
function pushTo(a, v) { return a.push(v); }
function popFrom(a) { return a.pop(); }
function shiftFrom(a) { return a.shift(); }
function unshiftTo(a, v) { return a.unshift(v); }
var arr: any = [1, 2, 3];
var len = pushTo(arr, 4);
var last = popFrom(arr);
var first = shiftFrom(arr);
var ulen = unshiftTo(arr, 0);
console.log(arr.join(',') + '|' + len + '|' + last + '|' + first + '|' + ulen);

// A statically-typed array keeps the dense fast path and correct semantics.
var typed: number[] = [];
typed.push(10);
typed.push(20, 30);
console.log(typed.join(',') + '|' + typed.length);
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
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
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout,
        // 1: object `push` closure ran for every chunk (was empty before the fix).
        // 2: object deque family (push/pop/shift/unshift) all dispatched to user methods.
        // 3: real array mutators via any-param: [0,2,3], push len 4, pop 4, shift 1, unshift len 3.
        // 4: typed-array fast path: [10,20,30], length 3.
        "<ul><li>a</li></ul>\nPUaPOSHUNb\n0,2,3|4|4|1|3\n10,20,30|3\n",
        "object push method must dispatch (not array-push); real arrays unaffected"
    );
}
