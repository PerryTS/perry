// Memory-leak regression for issue #1790 (epic #1785 / design #1772).
//
// A class EXPRESSION evaluates to a real heap "class object"
// (OBJECT_TYPE_CLASS): a regular object allocated per call, carrying its
// per-evaluation static fields as own properties. This loop creates and drops
// 1,000,000 of them, keeping only the latest in `sink`. Because each prior
// class object becomes unreachable once `sink` is reassigned, RSS must
// PLATEAU — the per-evaluation class objects must be reclaimed, not leaked.
//
// Catches regressions where:
//   - per-evaluation class objects are pinned or otherwise never collected
//   - the #1790 CLASS_PROTOTYPE_OBJECTS / CLASS_PARENT_CLOSURES rooting fix
//     over-roots class objects that were never registered as a parent. These
//     `make(i)` objects have no subclass and are never inserted into either
//     side-table, so the scanner is a no-op for them and they MUST be
//     reclaimed normally.
//
// The touch is deliberately GC-stale-safe: `sink = make(i)` keeps the result
// reachable (so dead-code elimination can't drop the allocation) without ever
// dereferencing the class-object pointer, and the asserted counter `kept` is a
// plain int. So the result stays deterministic across every GC mode regardless
// of allocation cadence. RSS (checked by the harness) is the leak signal.
//
// Run under all four GC modes (default / mark-sweep / gen-gc / force-evac+
// verify) by scripts/run_memory_stability_tests.sh.

declare function gc(): void;

function hasGc(): boolean {
  return typeof gc === "function";
}

function make(n: number) {
  return class {
    static count = n;
  };
}

let sink: any = null;
let kept = 0;
for (let i = 0; i < 1_000_000; i++) {
  sink = make(i);
  kept = i;
  if (i % 50_000 === 0 && hasGc()) {
    gc();
  }
}
// `sink !== null` touches the latest object without dereferencing it (a
// pointer/null bit compare), so the allocation can't be elided and the result
// can't be corrupted by a relocation.
console.log("done, kept=" + kept + " hasSink=" + (sink !== null));
