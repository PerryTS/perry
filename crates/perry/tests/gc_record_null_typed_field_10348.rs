//! #10348 — a closed-shape record field minted from a `null`-typed local is
//! left OUT of the class's compile-time GC pointer mask, so the collector
//! never scans that slot.
//!
//! `var head = null` infers `Type::Null`; the loop below then stores an object
//! into it. `typed_shape::type_is_pointer_bearing` answers `false` for
//! `Type::Null`, so the `next` slot of the synthesized `__AnonShape_*` record
//! was excluded from the mask that `js_gc_typed_shape_id_for_keys` registers
//! (#8405). Every allocation stamps `SIDE_MASK | TYPED_LAYOUT_INTACT` from the
//! baked header image, which means no per-object validation and no downgrade:
//! the chain hanging off `next` was neither marked nor rewritten, and the
//! program printed a truncated, cross-linked graph with exit code 0.
//!
//! **The check is `PERRY_GC_FROMSPACE_SCAN_ABORT`, not output parity.** That
//! scan walks every payload word of the whole heap and ignores layout state
//! entirely, so it sees the defect at the moment the collector drops the edge.
//! Output parity does not: with the same wrong mask and the `c.tag = …` store
//! removed, the 40 000-chain reproducer from the issue prints the *correct*
//! node count while the scan still reports 15 123 dangling references — the
//! dropped children simply had not been recycled into anything visible yet. A
//! test written against the printed answer would therefore have passed on a
//! heap that was already corrupt, so the assertion has to be the collector's
//! own whole-heap invariant. Parity with node is asserted too, as the symptom
//! the issue was filed for.
//!
//! Scale: 2 000 retained chains / 20 000 constructions is ~25 ms and aborts on
//! the first offender before the fix. The issue's 40 000-chain shape is what it
//! takes to reach OLD-PAGE evacuation and make `PERRY_GC_VERIFY_EVACUATION`
//! fire; that knob is silent at this size, which is exactly why the from-space
//! scan is the one armed here.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// A ring of retained 8-node chains, each node carrying a heap-pointer field
/// allocated with it (`payload`) and a later heap-pointer store (`tag`).
const RETAINED_CHAIN_RING: &str = r#"
var CHAINS = 2000, CHAIN_LEN = 8, TOTAL = 20000;

function makeChain(seed) {
  var head = null;
  for (var i = 0; i < CHAIN_LEN; i++) {
    head = { id: seed + i, payload: [seed, i], tag: null, next: head };
  }
  return head;
}

var ring = new Array(CHAINS);
for (var i = 0; i < CHAINS; i++) ring[i] = null;

for (var n = 0; n < TOTAL; n++) {
  var c = makeChain(n);
  c.tag = "n" + (n % 64);
  ring[n % CHAINS] = c;
}

var nodes = 0, truncated = 0;
for (var i = 0; i < CHAINS; i++) {
  var cur = ring[i], d = 0;
  while (cur !== null) { nodes++; cur = cur.next; d++; if (d > 50) break; }
  if (d !== CHAIN_LEN) truncated++;
}
console.log("nodes=" + nodes + " expected=" + (CHAINS * CHAIN_LEN) + " truncated=" + truncated);
"#;

#[test]
fn retained_chain_ring_keeps_every_next_edge_scannable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, RETAINED_CHAIN_RING).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    // The layout-blind whole-heap scan: aborts on the FIRST surviving word that
    // points into from-space, whatever the object's declared layout claims.
    let run = Command::new(&output)
        .current_dir(dir.path())
        .env("PERRY_GC_FROMSPACE_SCAN_ABORT", "1")
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "the from-space scan found a dropped edge (exit {:?}) — a record field \
         typed from a `null`-initialized local was left out of the class's GC \
         pointer mask (#10348)\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        stdout.contains("nodes=16000 expected=16000 truncated=0"),
        "every chain must still be intact and whole; got: {stdout}"
    );
}
