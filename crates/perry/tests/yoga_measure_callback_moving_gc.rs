//! A yoga measure callback must survive a copying minor that runs *during* the
//! layout pass, and the object it returns must survive the allocations made
//! while reading that object's fields.
//!
//! Two independent move-sensitive bugs lived in `perry-runtime/src/yoga.rs`,
//! both invisible to the evacuation verifier because neither offender is a slot
//! the collector walks:
//!
//! **(A) a snapshot the collector does not rewrite.** `js_yoga_calculate_layout`
//! built the taffy tree in phase 1 and, in the same pass, copied every leaf's
//! `node.measure` into a local `HashMap<u32, f64>`. Those `f64`s are NaN-boxed
//! JS closures. The registry's own copy is safe — `yoga_root_scanner` visits it
//! with `visit_nanbox_f64_slot(&mut ...)`, so an evacuating cycle rewrites it in
//! place — but a plain local `HashMap` is visited by nothing and rewritten by
//! nothing. A measure callback re-enters JS and may allocate, so the *first*
//! leaf that triggered a copying minor moved every closure and left the snapshot
//! holding from-space addresses for every leaf measured after it.
//!
//! That is also why the fault was regime-dependent: with in-place promotion the
//! stale address still happened to be valid, so it only appeared once
//! evacuating minors were the steady behaviour.
//!
//! **(B) a raw pointer held across an allocation.** `measure_leaf` took
//! `*const ObjectHeader` out of the returned NaN-boxed value and then called
//! `read_obj_number` twice. Each call mints a key string (`"width"`, `"height"`)
//! — an allocation, hence a possible copying minor — so the `width` read could
//! move the result object and the `height` read would dereference from-space.
//!
//! The knobs matter. `PERRY_GC_FORCE_EVACUATE` is read only on the MINOR path,
//! so this fixture must create real allocation pressure inside the callback
//! rather than call `gc()`; a `gc()`-driven probe here is a full mark-sweep that
//! moves nothing and would pass against both bugs.
//! `PERRY_GC_PROTECT_FROMSPACE` then makes a stale *use* fault at the point of
//! use instead of silently reading a stale copy that happens to still be intact.
//! It must be paired with `PERRY_GC_INSTRUMENTS=1` on the COMPILE step: the
//! instruments are in `default` features but the auto-optimize rebuild drops
//! them, and a binary without them aborts at startup rather than running with a
//! dead instrument. Both arms of this test's sabotage check abort identically if
//! that is forgotten, which reads exactly like a proven sabotage.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            perry_bin()
                .parent()
                .expect("compiler directory")
                .to_path_buf()
        })
}

/// Four leaves, each with its own measure callback returning distinct numbers.
/// Several leaves is the point: the bug needs one callback to collect and a
/// LATER callback to be invoked from the now-stale snapshot.
///
/// Only HEIGHT is asserted. `YogaNode::new` sets `FlexDirection::Column`, so
/// height is the main axis and comes straight from the measure result, while
/// width is the cross axis that flexbox may stretch to the container — an
/// assertion on width would fail for reasons that have nothing to do with this
/// bug.
const SOURCE: &str = r#"
// @ts-nocheck
import {
  nodeNew,
  insertChild,
  setMeasureFunc,
  calculateLayout,
  getComputed,
} from "perry/yoga"

const C_HEIGHT = 3

// Real allocation pressure, because PERRY_GC_FORCE_EVACUATE only arms minors.
function churn(n) {
  let sink = null
  for (let i = 0; i < n; i++) {
    sink = { i: i, tag: "yoga-churn-" + i, pad: [i, i + 1, i + 2] }
  }
  return sink
}

const root = nodeNew()
const leaves = []
const wantH = []
const seen = []
for (let i = 0; i < 4; i++) {
  const leaf = nodeNew()
  insertChild(root, leaf, i)
  leaves.push(leaf)
  const w = 10 + i * 7
  const h = 3 + i * 2
  wantH.push(h)
  setMeasureFunc(leaf, function (_w, _wm, _h, _hm) {
    churn(20000)
    seen.push(i)
    return { width: w, height: h }
  })
}

calculateLayout(root, NaN, NaN, 0)

// (A) every call must land on a REAL leaf closure, and each leaf must be
// measured at least once. Taffy measures a leaf more than once per layout (it
// ran each of these four times), so "exactly once" is wrong — what matters is
// that no call lands somewhere else, which is what a stale snapshot address
// does when it does not simply fault.
let callsOk = seen.length >= 4
for (let k = 0; k < seen.length; k++) {
  if (!(seen[k] >= 0 && seen[k] < 4)) callsOk = false
}
for (let i = 0; i < 4; i++) {
  if (seen.indexOf(i) < 0) callsOk = false
}

// (B) the height each callback returned must survive the two field reads.
let bad = 0
for (let i = 0; i < leaves.length; i++) {
  const gotH = getComputed(leaves[i], C_HEIGHT)
  if (gotH !== wantH[i]) {
    bad++
    console.log("leaf " + i + " height " + gotH + " want " + wantH[i])
  }
}
console.log("calls " + (callsOk ? "OK" : "WRONG [" + seen.join(",") + "]"))
console.log(bad === 0 ? "MEASURED OK" : "MEASURED WRONG " + bad)
"#;

#[test]
fn measure_callbacks_survive_an_evacuating_minor_mid_layout() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("main.ts"), SOURCE).unwrap();

    let output = root.join("yoga_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(root.join("main.ts"))
        .arg("-o")
        .arg(&output)
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        // The instruments live in perry-runtime's `default` features, but the
        // auto-optimize rebuild PRUNES them unless an instrument knob (or this
        // flag) is set at COMPILE time. Without it the binary aborts at startup
        // the moment `PERRY_GC_PROTECT_FROMSPACE` is set at run time — which is
        // deliberate: an instrument that silently did nothing would turn this
        // gate green against both bugs.
        .env("PERRY_GC_INSTRUMENTS", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(root)
        .env("PERRY_GC_FORCE_EVACUATE", "1")
        .env("PERRY_GC_PROTECT_FROMSPACE", "1")
        .output()
        .expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);

    assert!(
        run.status.success(),
        "the layout pass must not fault: pre-fix a measure callback was invoked \
         through a from-space address left in the phase-1 snapshot, and the \
         measure result was read through a pointer captured before a key-string \
         allocation\nstatus: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        run.status
    );
    assert!(
        stdout.contains("calls OK"),
        "every measure call must land on a real leaf closure and every leaf must \
         be measured: a phase-1 snapshot left later leaves calling a from-space \
         address\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("MEASURED OK"),
        "every leaf must report the height its own measure callback returned\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
