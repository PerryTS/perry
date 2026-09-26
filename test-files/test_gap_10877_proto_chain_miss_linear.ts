// #10877: an absent read walks each prototype ONCE. The own-key-miss path
// read the receiver's prototype twice — for an `Object.create` chain, the
// recorded chain walked twice; for a `new F()` chain, `F.prototype` read once
// through F's class id and once through the instance's prototype link — and
// each read re-enters the generic getter one level up, so a miss cost 2^depth
// getter entries. An inherited getter that returns `undefined` is a miss to
// those walks, so it ran once per re-entry: the call counts below are exact
// where the cost was only a slowdown.

let calls = 0;
const root: any = {
  get probe() {
    calls++;
    return undefined;
  },
  present: "root-value",
};

function chain(depth: number): any {
  let p: any = root;
  for (let i = 0; i < depth; i++) p = Object.create(p);
  return p;
}

// ── 1. an inherited getter answering undefined runs once per read ───────────
for (const depth of [1, 2, 3, 5, 8]) {
  const o = chain(depth);
  calls = 0;
  const v = o.probe;
  console.log("getter", depth, String(v), calls);
}

// ── 2. the same with an own key on the receiver (shaped receiver) ────────────
for (const depth of [1, 3, 8]) {
  const o = chain(depth);
  o.own = 1;
  calls = 0;
  const v = o.probe;
  console.log("shaped", depth, String(v), calls);
}

// ── 3. hits and misses still resolve at every depth ─────────────────────────
for (const depth of [1, 4, 12]) {
  const o = chain(depth);
  console.log("hit", depth, o.present, String(o.absent), "absent" in o);
}

// ── 4. a deep miss finishes: 2^depth entries would not ──────────────────────
{
  const o = chain(24);
  let misses = 0;
  for (let i = 0; i < 1000; i++) if (o.nothingHere === undefined) misses++;
  console.log("deep", misses, o.present);
}

// ── 5. constructor-function chains: `F.prototype = new G()` ────────────────
function F0(this: any) {}
(F0 as any).prototype = root;
function F1(this: any) {}
(F1 as any).prototype = new (F0 as any)();
function F2(this: any) {}
(F2 as any).prototype = new (F1 as any)();
function F3(this: any) {}
(F3 as any).prototype = new (F2 as any)();
(F3 as any).prototype.nullish = null;
for (const C of [F0, F1, F2, F3]) {
  const o = new (C as any)();
  calls = 0;
  const v = o.probe;
  const keyless = calls;
  o.own = 1;
  calls = 0;
  o.probe;
  console.log("ctor", C.name, String(v), keyless, calls, o.present, String(o.absent));
}
{
  const o = new (F3 as any)();
  console.log("ctor-null", o.nullish === null, "nullish" in o);
  o.own = 1;
  console.log("ctor-null-shaped", o.nullish === null);
}
{
  let Prev: any = F0;
  for (let q = 0; q < 24; q++) {
    const P = Prev;
    const G: any = function (this: any) {};
    G.prototype = new P();
    Prev = G;
  }
  const o = new Prev();
  let misses = 0;
  for (let i = 0; i < 1000; i++) if (o.nothingHere === undefined) misses++;
  console.log("ctor-deep", misses, o.present);
}

// ── 6. a middle link and Object.prototype are still consulted ───────────────
{
  const mid: any = Object.create(chain(3));
  mid.midValue = "mid";
  const leaf: any = Object.create(Object.create(mid));
  console.log("mid", leaf.midValue, leaf.present, typeof leaf.hasOwnProperty);
  (Object.prototype as any).fromObjectProto = "op";
  console.log("objproto", leaf.fromObjectProto);
  delete (Object.prototype as any).fromObjectProto;
  console.log("objproto-gone", String(leaf.fromObjectProto));
}
