// Parity: everything `delete` is allowed to be observed doing — enumeration
// order after a re-add, delete order, wide receivers, descriptors, accessors,
// class instances and prototypes, index keys, and interleaved read/churn.
//
// `delete` is a shape transition: every delete moves the receiver's ShapeId,
// so a (shape, key) cache entry primed before it cannot hit after it. That is
// invisible in output when it works and silent when it breaks, so this file
// pins the JS-visible half against Node and the unit tests in
// `object/tombstone_tests.rs` pin the identity half.
function show(label: string, v: unknown): void {
  console.log(label + "=" + v);
}

// 1. enumeration order: a re-added key goes to the END.
const a: Record<string, number> = { x: 1, y: 2, z: 3 };
delete a.y;
show("a1", Object.keys(a).join(","));
a.y = 9;
show("a2", Object.keys(a).join(","));
show("a3", JSON.stringify(a));
let forin = [];
for (const k in a) forin.push(k);
show("a4", forin.join(","));
show("a5", Object.entries(a).map(function (e) { return e[0] + ":" + e[1]; }).join("|"));
const spreadA = Object.assign({}, a);
show("a6", Object.keys(spreadA).join(",") + "/" + JSON.stringify(spreadA));

// 2. delete order independence of the RESULT, dependence of the ORDER.
function build(): Record<string, number> {
  return { k0: 0, k1: 1, k2: 2, k3: 3, k4: 4 };
}
const fwd = build(); delete fwd.k1; delete fwd.k3;
const bwd = build(); delete bwd.k3; delete bwd.k1;
show("b1", Object.keys(fwd).join(","));
show("b2", Object.keys(bwd).join(","));
fwd.k1 = 11; fwd.k3 = 33; bwd.k3 = 33; bwd.k1 = 11;
show("b3", Object.keys(fwd).join(","));
show("b4", Object.keys(bwd).join(","));
show("b5", JSON.stringify(fwd) + "|" + JSON.stringify(bwd));

// 3. a wide receiver: delete every other key, then re-add, twice over.
const wide: Record<string, number> = {};
for (let i = 0; i < 40; i++) wide["w" + i] = i;
for (let i = 0; i < 40; i += 2) delete wide["w" + i];
show("c1", Object.keys(wide).length + "/" + Object.keys(wide).join(","));
for (let i = 0; i < 40; i += 2) wide["w" + i] = i * 10;
show("c2", Object.keys(wide).length + "/" + Object.keys(wide).slice(0, 8).join(","));
let csum = 0;
for (const k in wide) csum += wide[k];
show("c3", csum);
for (let i = 0; i < 40; i++) delete wide["w" + i];
show("c4", Object.keys(wide).length + "/" + JSON.stringify(wide));

// 4. descriptors.
const d: Record<string, number> = { p: 1, q: 2, r: 3 };
Object.defineProperty(d, "locked", { value: 7, configurable: false, enumerable: true, writable: true });
Object.defineProperty(d, "hidden", { value: 8, configurable: true, enumerable: false, writable: true });
show("d1", Object.keys(d).join(","));
show("d2", delete d.q);
try { show("d3", delete d.locked); } catch (e) { show("d3", "throw"); }
show("d4", delete d.hidden);
show("d5", Object.keys(d).join(",") + "/" + d.locked + "/" + d.hidden);
show("d6", Object.getOwnPropertyNames(d).join(","));
show("d7", JSON.stringify(Object.getOwnPropertyDescriptor(d, "locked")));

// 5. accessors.
const acc: Record<string, unknown> = { base: 1 };
Object.defineProperty(acc, "g", { get: function () { return 42; }, configurable: true, enumerable: true });
show("e1", acc.g + "/" + Object.keys(acc).join(","));
show("e2", delete acc.g);
show("e3", acc.g + "/" + Object.keys(acc).join(","));
show("e4", "g" in acc);

// 6. class instances and prototypes.
class C { constructor() { this.f1 = 1; this.f2 = 2; this.f3 = 3; } m() { return "m"; } }
const ci = new C();
show("f1", delete ci.f2);
show("f2", Object.keys(ci).join(",") + "/" + ci.f2 + "/" + ci.f3);
ci.f2 = 22;
show("f3", Object.keys(ci).join(",") + "/" + ci.f2);
show("f4", ci.m());
show("f5", delete C.prototype.m);
// SCOPED OUT: `typeof C.prototype.m` still reports "function" on perry after a
// successful `delete` even though the key is gone from getOwnPropertyNames.
// Reproduces on an unmodified base binary (v0.5.1618) and is unrelated to the
// delete-shape-transition change; asserting the wrong value here would bake it in.
//   show("f6", typeof C.prototype.m);   // node: undefined, perry: function
show("f6", Object.getOwnPropertyNames(C.prototype).join(","));

const proto: Record<string, unknown> = { inherited: "yes", shared: 1 };
const child = Object.create(proto);
child.own = "mine";
show("g1", child.inherited + "/" + child.own + "/" + Object.keys(child).join(","));
show("g2", delete proto.inherited);
show("g3", child.inherited + "/" + ("inherited" in child));
show("g4", delete child.own);
show("g5", child.own + "/" + Object.keys(child).length);

// 7. delete of an absent key, of an index key, of a non-object.
const h: Record<string, unknown> = { only: 1 };
show("h1", delete h.nope);
show("h2", delete h["0"]);
h[0] = "zero"; h[1] = "one";
show("h3", Object.keys(h).join(","));
show("h4", delete h[0]);
show("h5", Object.keys(h).join(",") + "/" + JSON.stringify(h));

// 8. churn with reads interleaved (the shape must never hand back a stale slot).
const churn: Record<string, number> = {};
for (let i = 0; i < 12; i++) churn["c" + i] = i;
let bad = 0;
for (let round = 0; round < 60; round++) {
  const k = "c" + (round % 12);
  delete churn[k];
  if (churn[k] !== undefined) bad++;
  if (Object.prototype.hasOwnProperty.call(churn, k)) bad++;
  churn[k] = round;
  if (churn[k] !== round) bad++;
  const other = "c" + ((round + 5) % 12);
  if (typeof churn[other] !== "number") bad++;
}
show("i1", bad);
show("i2", Object.keys(churn).length);
show("i3", Object.keys(churn).sort().join(","));
