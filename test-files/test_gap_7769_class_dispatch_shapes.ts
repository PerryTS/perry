// #7769: virtual dispatch, `instanceof` and the class parent chain stopped
// going through a lock + SipHash probe per hop (a dense parent mirror), and
// `js_native_call_method` grew a cache of its own resolution so an ordinary
// class instance skips the dispatch tower.
//
// Both changes touch class REGISTRATION and the precedence rules the tower
// encodes, so this pins the shapes CLAUDE.md flags as weak for native
// base-class subclassing — a fieldless subclass, a two-level indirect
// subclass, a class EXPRESSION (including one with an `extends`), and
// dispatch through a base-typed collection — plus the precedence rule the
// dispatch fast path must not break: an own field shadows a vtable method,
// including after that (class, method name) pair has already been cached.

class Base {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
  kind(): string {
    return "base";
  }
  score(): number {
    return this.x;
  }
}

// A fieldless subclass — no own state at all.
class Marker extends Base {
  kind(): string {
    return "marker";
  }
}

// A two-level indirect subclass: Leaf -> Mid -> Base.
class Mid extends Base {
  y: number;
  constructor(x: number, y: number) {
    super(x);
    this.y = y;
  }
  kind(): string {
    return "mid";
  }
  score(): number {
    return this.x + this.y;
  }
}
class Leaf extends Mid {
  kind(): string {
    return "leaf";
  }
}

// A subclass that overrides NOTHING — inherits both methods across two hops.
class Silent extends Mid {}

// A class expression, and a class expression WITH an extends clause.
const Anon = class {
  kind(): string {
    return "anon";
  }
  score(): number {
    return 7;
  }
};
const AnonSub = class extends Base {
  kind(): string {
    return "anonsub";
  }
};

// ── 1. dispatch through a base-typed collection ──
const items: Base[] = [
  new Base(1),
  new Marker(2),
  new Mid(3, 4),
  new Leaf(5, 6),
  new Silent(7, 8),
  new AnonSub(9),
];
let kinds = "";
let total = 0;
for (let i = 0; i < items.length; i++) {
  kinds = kinds + items[i].kind() + ",";
  total = total + items[i].score();
}
console.log("1 kinds:", kinds);
console.log("1 total:", total);

// Repeat the whole loop so every call site runs both cold (tower) and warm
// (cached resolution) — a cache that answered a later iteration differently
// would show up right here.
let kinds2 = "";
for (let r = 0; r < 3; r++) {
  for (let i = 0; i < items.length; i++) kinds2 = kinds2 + items[i].kind();
}
console.log("2 warm kinds stable:", kinds2 === (kinds.split(",").join("")).repeat(3));

// ── 3. instanceof across the whole lattice ──
const leaf = new Leaf(1, 2);
const marker = new Marker(3);
console.log("3 leaf:", leaf instanceof Leaf, leaf instanceof Mid, leaf instanceof Base);
console.log("3 marker:", marker instanceof Marker, marker instanceof Base, marker instanceof Mid);
console.log("3 anon:", new Anon() instanceof Anon, new AnonSub(1) instanceof Base);
console.log("3 negatives:", leaf instanceof Marker, marker instanceof Leaf);

// ── 4. an OWN field shadows the vtable method ──
// The dispatch fast path re-scans own keys precisely so this keeps working
// after the same (class, method name) pair has been resolved to the vtable.
const shadowed: any = new Base(10);
console.log("4 before shadow:", shadowed.kind());
shadowed.kind = () => "own-field";
console.log("4 after shadow:", shadowed.kind());
const fresh: any = new Base(11);
console.log("4 sibling unaffected:", fresh.kind());

// NOTE — two shapes deliberately NOT asserted here, because Perry already
// diverges from Node on them at this change's merge-base (verified by running
// this file's earlier draft against a binary built from `origin/main`, which
// produced the identical wrong answers):
//
//   * `Class.prototype.m = fn` after the first dispatch of `m` still resolves
//     to the vtable method (Node: the assigned one);
//   * `Object.setPrototypeOf(instance, donor)` does not redirect an already
//     dispatched method on that instance (Node: it does).
//
// Both are upstream of the dispatch cache: the fast path's guard REJECTS a
// receiver with a non-null `meta` record (which `setPrototypeOf` installs) and
// prototype surgery now bumps `VTABLE_GEN`, so neither is reached from cache —
// the tower produces these answers on its own. Asserting them here would make
// this file a permanent gap failure and hide the regressions it exists to
// catch, so they are left to whoever fixes the tower.

// ── 7. super() chains and statics still resolve up the (now dense) chain ──
class SBase {
  static made: number = 0;
  constructor() {
    SBase.made = SBase.made + 1;
  }
  static describe(): string {
    return "sbase";
  }
}
class SMid extends SBase {}
class SLeaf extends SMid {}
new SLeaf();
new SMid();
new SBase();
console.log("7 statics:", SBase.made, SLeaf.describe(), SMid.describe());

// ── 8. a computed method name reaches the same dispatch ──
// This is the path that materialises the method name into a runtime string
// rather than a rodata constant, so it exercises the cache's content keying.
const names = ["kind", "score"];
let computed = "";
for (let i = 0; i < items.length; i++) {
  computed = computed + String((items[i] as any)[names[i % 2]]()) + "|";
}
console.log("8 computed:", computed);
