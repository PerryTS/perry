// Representation-selection Phase 5a soundness: `Object.freeze` / `seal` /
// `preventExtensions` vs. a proven `this` (PERRY_PTR_SHAPE_THIS, RFC
// docs/representation-selection-rfc.md §5.2/§5.7).
//
// Why this file is separate from test_gap_repsel_ptr_shape_locals.ts: the
// freeze-family kill is MODULE-WIDE. A Phase 3b `Ptr<Shape>` local needs no
// such rule — its containment walk proves no alias to the object exists, and
// freezing the local itself disqualifies it directly. A proven `this` has no
// containment: the receiver is owned by the CALLER and is therefore aliased by
// construction, so `Object.freeze(c)` followed by `c.m()` would let a
// guard-free raw store silently succeed where the spec requires a strict-mode
// TypeError. Any freeze-family site in the module therefore disables
// proven-`this` clones that contain a `this.field` WRITE; read-only clones are
// unaffected.
//
// Class methods are strict-mode code, so a write to a frozen own property
// THROWS rather than silently no-opping. That is the observable this file
// pins byte-for-byte against Node.

class Frozen {
  value: number;
  label: string;
  constructor(value: number, label: string) {
    this.value = value;
    this.label = label;
  }
  // WRITE-containing method: the clone for this one is what the freeze kill
  // must suppress.
  bump(by: number): number {
    this.value = this.value + by;
    return this.value;
  }
  // READ-only method: always safe — a frozen object reads back exactly the
  // same slot bits.
  read(): number {
    return this.value * 2;
  }
  describe(): string {
    return this.label + "=" + this.value;
  }
}

// 1. Un-frozen receiver: writes land normally.
const live = new Frozen(10, "live");
console.log("live:" + live.bump(5) + ":" + live.read() + ":" + live.describe());

// 2. Frozen receiver, strict-mode method body: the write MUST throw a
//    TypeError. A guard-free raw store would have silently succeeded.
const frost = new Frozen(1, "frost");
Object.freeze(frost);
console.log("frozen-read:" + frost.read() + ":" + frost.describe());
try {
  frost.bump(1);
  console.log("frozen-write: NO THROW (wrong)");
} catch (e) {
  console.log("frozen-write:" + (e instanceof TypeError) + ":" + (e as Error).name);
}
// The value must be unchanged after the failed write.
console.log("frozen-after:" + frost.value + ":" + frost.read());

// 3. Frozen through an ALIAS — the shape the containment walk cannot see.
//    `alias` and `target` are the same object; freezing through one must be
//    observed by a method call through the other.
const target = new Frozen(100, "t");
const alias = target;
Object.freeze(alias);
try {
  target.bump(7);
  console.log("alias-write: NO THROW (wrong)");
} catch (e) {
  console.log("alias-write:" + (e instanceof TypeError));
}
console.log("alias-after:" + target.value + ":" + alias.value);

// 4. `Object.seal` — existing properties stay writable, so the write lands;
//    only additions are rejected. Reads and writes must both be exact.
const sealed = new Frozen(3, "s");
Object.seal(sealed);
console.log("sealed-write:" + sealed.bump(4) + ":" + sealed.describe());
console.log("sealed-frozen:" + Object.isFrozen(sealed) + ":" + Object.isSealed(sealed));

// 5. `Object.preventExtensions` — same: existing fields remain writable.
const noext = new Frozen(20, "n");
Object.preventExtensions(noext);
console.log("noext-write:" + noext.bump(2) + ":" + Object.isExtensible(noext));

// 6. A hot loop over a frozen receiver's READ-only method: this is the path
//    that stays guard-free even under the module-wide write kill.
function readLoop(n: number): number {
  let acc = 0;
  for (let i = 0; i < n; i++) {
    acc += frost.read();
  }
  return acc;
}
console.log("read-loop:" + readLoop(1000));

// 7. Freeze AFTER a hot write loop: the clone (if any) and the guarded path
//    must agree on the final value, and the post-freeze write must throw.
const late = new Frozen(0, "late");
for (let i = 0; i < 500; i++) {
  late.bump(1);
}
console.log("late-before:" + late.value);
Object.freeze(late);
try {
  late.bump(1);
  console.log("late-write: NO THROW (wrong)");
} catch (e) {
  console.log("late-write:" + (e instanceof TypeError));
}
console.log("late-after:" + late.value + ":" + late.describe());
