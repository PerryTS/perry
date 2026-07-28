// Representation-selection Phase 4b.1/4b.2: class-field store bookkeeping.
// (RFC docs/representation-selection-rfc.md §5.7.)
//
// 4b.1 retires `js_gc_note_slot_layout` and `js_string_addref_if_heap_string`
// on Ptr<Shape>-proven class-field stores where each is provably dead. This
// file pins the OBSERVABLE behaviour of both the elided and the deliberately
// NON-elided cases — every section must stay byte-identical to Node.
//
// The addref cases are the sharp edge: `js_string_addref_if_heap_string`
// demotes a uniquely-owned (refcount==1) heap string to shared so a later
// in-place `+=` on the source allocates fresh instead of rewriting the stored
// field underneath it. Eliding it wrongly is SILENT corruption, so each such
// case builds a genuinely non-SSO (> 5 byte), genuinely unique (first append on
// a shared literal) string, stores it, then grows the source.

// 1. Elided both: a `boolean` field is neither raw-f64 nor pointer-bearing, so
//    its slot is in neither typed mask; a by-construction non-pointer value
//    (literal / comparison / `!`) makes both calls dead.
class Flags {
  on: boolean;
  hot: boolean;
  seen: boolean;
  constructor() {
    this.on = false;
    this.hot = false;
    this.seen = false;
  }
}
function boolStores(n: number): string {
  const f = new Flags();
  let flips = 0;
  for (let i = 0; i < n; i++) {
    f.on = true;
    f.hot = i > n / 2;
    f.seen = !f.on;
    if (f.hot) flips++;
  }
  return f.on + "," + f.hot + "," + f.seen + "," + flips;
}
console.log(boolStores(1000));

// 2. Elided note, kept addref: a `string` field's slot IS pointer-masked, so
//    the layout note is a no-op — but the stored value can be a unique heap
//    string, so the demote must survive.
class Named {
  tag: string;
  constructor() {
    this.tag = "";
  }
}
function stringFieldKeepsAddref(): string {
  const o = new Named();
  let s = "prefix"; // 6 bytes -> heap (non-SSO), shared literal
  s += "_init"; // append on shared -> fresh heap string, refcount==1
  o.tag = s; // MUST demote s to shared
  s += "_more"; // refcount==1 in-place append must NOT rewrite o.tag
  return "tag=" + o.tag + " s=" + s;
}
console.log(stringFieldKeepsAddref());

// 3. Snapshot-then-grow across two proven receivers: each stored snapshot must
//    keep the value it had at store time.
function snapshots(): string {
  const a = new Named();
  const b = new Named();
  let cur = "prefix";
  cur += "_one";
  a.tag = cur;
  cur += "_two";
  b.tag = cur;
  cur += "_three";
  return "a=" + a.tag + " b=" + b.tag + " cur=" + cur;
}
console.log(snapshots());

// 4. Union-with-string must NOT elide the demote: `type_is_pointer_bearing`
//    reports the slot as pointer-masked (so the note goes), but a string can
//    still land there and must be demoted.
class Mixed {
  v: string | number;
  constructor() {
    this.v = 0;
  }
}
function unionFieldKeepsAddref(): string {
  const m = new Mixed();
  let s = "prefix";
  s += "_union";
  m.v = s;
  s += "_grown";
  const first = "" + m.v;
  m.v = 42; // non-pointer store into the same pointer-masked slot
  return "first=" + first + " s=" + s + " then=" + m.v + " t=" + typeof m.v;
}
console.log(unionFieldKeepsAddref());

// 5. Declared types are NOT enforced at runtime: a `boolean`-declared field can
//    receive a string smuggled through `any`. The demote is gated on the VALUE
//    expression, never on the declared type, so this must stay correct.
class Loose {
  flag: boolean;
  n: number;
  constructor() {
    this.flag = false;
    this.n = 0;
  }
}
function declaredTypeIsNotEnforced(): string {
  const l = new Loose();
  let s = "prefix";
  s += "_smuggled";
  l.flag = s as any; // a string in a `boolean` slot — must still demote
  s += "_after";
  return "flag=" + l.flag + " s=" + s + " t=" + typeof l.flag;
}
console.log(declaredTypeIsNotEnforced());

// 6. Object- and array-typed fields: pointer-masked slots, so the note is
//    elided; values stay exact and the children stay reachable across GC.
class Node2 {
  id: number;
  next: Node2 | null;
  constructor(id: number) {
    this.id = id;
    this.next = null;
  }
}
class Holder {
  head: Node2 | null;
  nums: number[];
  label: string;
  constructor() {
    this.head = null;
    this.nums = [];
    this.label = "";
  }
}
function pointerFields(n: number): string {
  const h = new Holder();
  let total = 0;
  for (let i = 0; i < n; i++) {
    // Fresh child each iteration: only the field write keeps it alive, so a
    // wrongly-elided layout note would strand it for the collector.
    h.head = new Node2(i);
    h.nums = [i, i + 1, i + 2];
    h.label = "n" + i;
    total += h.head.id + h.nums[2] + h.label.length;
  }
  return total + ":" + h.head!.id + ":" + h.nums.join("-") + ":" + h.label;
}
console.log(pointerFields(2000));

// 7. Null-out then re-point: the pointer-masked slot goes non-pointer and back.
function nullOutAndRepoint(): string {
  const h = new Holder();
  h.head = new Node2(7);
  const seen1 = h.head.id;
  h.head = null; // non-pointer by construction into a pointer-masked slot
  const seen2 = h.head === null;
  h.head = new Node2(9);
  return seen1 + "," + seen2 + "," + h.head.id;
}
console.log(nullOutAndRepoint());

// 8. 4b.2 — INT32-boxed numerics reaching a typed `number` field must not
//    change what is read back. Integers that round-trip through JSON (the
//    sqlite/v8-IPC shape that motivated the fix) stay integers, not null.
class Row {
  id: number;
  count: number;
  ratio: number;
  constructor(id: number, count: number, ratio: number) {
    this.id = id;
    this.count = count;
    this.ratio = ratio;
  }
}
function int32Fields(): string {
  const r = new Row(0, 0, 0);
  let acc = 0;
  for (let i = 0; i < 500; i++) {
    r.id = i | 0; // bitwise -> integral
    r.count = (i * 3) | 0;
    r.ratio = i / 4;
    acc += r.id + r.count + r.ratio;
  }
  return (
    acc +
    ":" +
    JSON.stringify(r) +
    ":" +
    (r.id | 0) +
    ":" +
    Object.is(r.count, 1497) +
    ":" +
    r.ratio.toFixed(2)
  );
}
console.log(int32Fields());

// 9. Mixed sequence on one receiver: interleave every mask class so any
//    descriptor bookkeeping mistake shows up as a wrong read rather than a
//    crash-free-but-silent divergence.
class Everything {
  num: number;
  flag: boolean;
  text: string;
  list: number[];
  constructor() {
    this.num = 0;
    this.flag = false;
    this.text = "";
    this.list = [];
  }
}
function interleaved(n: number): string {
  const e = new Everything();
  let sum = 0;
  for (let i = 0; i < n; i++) {
    e.num = i * 1.5;
    e.flag = (i & 1) === 0;
    e.text = "v" + (i % 10);
    e.list = [i];
    sum += e.num + (e.flag ? 1 : 0) + e.text.length + e.list[0];
  }
  return sum + "|" + e.num + "|" + e.flag + "|" + e.text + "|" + e.list[0];
}
console.log(interleaved(1500));

// 10. Same shapes under allocation pressure so real minor collections run
//     between the field writes and the reads.
function underGcPressure(n: number): string {
  const h = new Holder();
  let live = 0;
  for (let i = 0; i < n; i++) {
    const garbage = new Array(32).fill(i); // allocation safepoint
    h.head = new Node2(garbage[i % 32]);
    h.label = "g" + (i % 7);
    h.nums = [garbage.length];
    live += h.head.id % 3;
  }
  return live + ":" + h.head!.id + ":" + h.label + ":" + h.nums[0];
}
console.log(underGcPressure(3000));

// 11. THE STORE SITE 4b.1 ACTUALLY CHANGES. A plain `o.f = v` on a local
//     lowers to `PutValueSet` (the PutValue IC); it is compound and logical
//     assignment (`+=`, `||=`, `??=`) that lowers to a `PropertySet` on a
//     `LocalGet` receiver, which is the Ptr<Shape>-proven class-field store
//     path. One case per mask class, plus the addref boundary.
class Slot {
  tag: string;
  child: Node2 | null;
  nums: number[] | null;
  flag: boolean;
  count: number;
  constructor() {
    this.tag = "";
    this.child = null;
    this.nums = null;
    this.flag = false;
    this.count = 0;
  }
  size(): number {
    return this.tag.length;
  }
}

function compoundStores(): string {
  const o = new Slot();
  let s = "prefix"; // non-SSO
  s += "_init"; // append on a shared literal -> refcount==1
  o.tag ||= s; // pointer-masked slot: note elided, addref MUST stay
  s += "_more"; // must NOT rewrite o.tag
  o.child ??= new Node2(3); // pointer-masked, pointer RHS
  o.nums ??= [1, 2]; // pointer-masked, pointer RHS
  o.flag ||= true; // neither mask + non-pointer RHS -> both elided
  o.count += 5; // raw-f64 slot: separate inline path, untouched
  return (
    o.tag +
    "|" +
    s +
    "|" +
    o.child.id +
    "|" +
    o.nums.join(",") +
    "|" +
    o.flag +
    "|" +
    o.count +
    "|" +
    o.size()
  );
}
console.log(compoundStores());

// 12. The changed store in a hot loop under allocation pressure: the elided
//     layout note has to hold up across real collections while the
//     pointer-masked slots keep freshly allocated children alive.
function compoundLoop(n: number): string {
  const o = new Slot();
  let live = 0;
  for (let i = 0; i < n; i++) {
    const garbage = new Array(16).fill(i); // allocation safepoint
    o.child = null; // clear so `??=` re-points every iteration
    o.child ??= new Node2(garbage[i % 16]);
    o.nums = null;
    o.nums ??= [i];
    o.flag ||= i === n - 1;
    o.count += 1;
    live += (o.child.id % 5) + (o.nums[0] % 3);
  }
  return (
    live + ":" + o.child!.id + ":" + o.nums![0] + ":" + o.flag + ":" + o.count
  );
}
console.log(compoundLoop(3000));

// 13. The addref boundary inside the changed store, repeated: each snapshot
//     taken with `||=` must keep the value it had when stored, while the
//     source keeps growing.
function compoundSnapshots(): string {
  const a = new Slot();
  const b = new Slot();
  let cur = "prefix";
  cur += "_one";
  a.tag ||= cur;
  cur += "_two";
  b.tag ||= cur;
  cur += "_three";
  return "a=" + a.tag + " b=" + b.tag + " cur=" + cur;
}
console.log(compoundSnapshots());
