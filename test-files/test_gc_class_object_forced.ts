// Forced-GC correctness regression for issue #1790 (epic #1785 / design #1772).
//
// `class Sub extends make(...) {}` records the parent class OBJECT (a class
// expression value) in the CLASS_PROTOTYPE_OBJECTS side-table so static-field
// and static-method inheritance can walk to it. Before #1790 that parent was
// stored as a raw pointer the GC could neither see nor relocate: under a
// collection it could be SWEPT (the only reference lives in the side-table) or
// DANGLE after a copying-nursery / C4b evacuation moved it. Either way the
// inheritance walk (`Sub.label`, `Sub.describe()`) would read freed/stale
// memory.
//
// This test forces aggressive GC (the harness runs it under
// PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1) AFTER the subclasses
// are defined, then asserts every inherited static field/method still resolves
// to the correct value. Pass ⟺ exit 0 + the exact summary line, identical to
// Node (where `gc` is undefined, so forceGc is a no-op).
//
// Covers, in one program:
//   - mixed pointer + scalar static fields surviving GC (string, object, int)
//   - inherited static method resolving after evacuation
//   - a two-level inheritance chain (Leaf -> Mid -> class object)

declare function gc(): void;

function forceGc(): void {
  if (typeof gc === "function") {
    gc();
  }
}

function makeBase(tag: string, n: number) {
  return class {
    static label = tag; // pointer static field (string)
    static meta = { kind: tag, n: n }; // pointer static field (object)
    static num = n; // scalar static field (int)
    static describe(): string {
      // Read state via `this` (the proven inherited-static-method pattern):
      // for a subclass this walks the CLASS_PROTOTYPE_OBJECTS chain to the
      // parent class object's static fields — exactly the rooted+rewritten
      // pointer. If the parent dangled after evacuation, this would read
      // garbage rather than "alpha"/"mid".
      return "d:" + (this as any).label + ":" + (this as any).num;
    }
  };
}

// The parent class object here is reachable ONLY through CLASS_PROTOTYPE_OBJECTS.
class Mixed extends makeBase("alpha", 7) {}

// Allocation pressure + forced GC: under force-evac the parent class object is
// evacuated and its side-table pointer must be rewritten.
const filler: any[] = [];
for (let i = 0; i < 20_000; i++) {
  filler.push({ x: i, s: "f_" + i });
}
forceGc();
forceGc();

// Two-level inheritance: Leaf -> Mid -> (class object).
class Mid extends makeBase("mid", 3) {}
class Leaf extends Mid {}
for (let i = 0; i < 20_000; i++) {
  filler.push({ y: i });
}
forceGc();

const out =
  (Mixed as any).label +
  "|" +
  (Mixed as any).meta.kind +
  "|" +
  (Mixed as any).meta.n +
  "|" +
  (Mixed as any).num +
  "|" +
  (Mixed as any).describe() +
  "|" +
  (Leaf as any).label +
  "|" +
  (Leaf as any).describe();

console.log("done: " + out);
