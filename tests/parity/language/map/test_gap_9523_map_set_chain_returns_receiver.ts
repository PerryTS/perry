// #9523 (second item): `lower_call/property_get/map_set.rs`'s `"set"` arm — the
// path `this.field.set(k, v)` takes when the field is declared `Map<K, V>` —
// called `js_map_set` as `void` and returned the receiver box it had read from
// its root slot BEFORE the call. `Expr::MapSet` (the `m.set(k, v)` shape on a
// plain local) re-boxes the pointer the helper RETURNS instead.
//
// `js_map_set` returns the receiver as it stands after the insert. When the
// receiver is a `class X extends Map` instance, the runtime resolves it to the
// hidden backing `MapHeader`, roots the receiver (a movable `ObjectHeader`) and
// runs the insert under that root (`map_op_returning_receiver`, #7570). A
// moving minor inside the grow (`ensure_capacity` notes an external side
// allocation, which can trigger one) relocates the receiver, and the helper
// hands back the NEW address. The pre-call box is then a from-space pointer —
// and a chained `.set(a, 1).set(b, 2)` is exactly the consumer of that value:
// the second call dispatches on whatever the first one returned.
//
// The fault needs the minor to fire INSIDE the first `set`'s grow, which is a
// pressure question rather than a deterministic one, so this fixture sweeps
// the nursery fill across rounds and reports how many rounds disagree with
// the specification. Node prints `bad=0` for every round.

class Registry extends Map<string, number> {}

class Holder {
  m: Map<string, number>;
  constructor() {
    this.m = new Registry();
  }
  // The chained shape. The FIRST `.set` is the `"set"` arm; the second one
  // consumes its return value.
  put(a: string, b: string): number {
    this.m.set(a, 1).set(b, 2);
    return this.m.size;
  }
  // The identity contract on its own: `Map.prototype.set` returns `this`.
  same(a: string): boolean {
    return this.m.set(a, 3) === this.m;
  }
}

// Allocates `n` escaping cells (kept alive in a bounded window) so the nursery
// is filled to a controlled level before the chained set runs.
function fill(n: number): any[] {
  let keep: any[] = [];
  for (let i = 0; i < n; i++) {
    keep.push({ a: i, b: i + 1, c: i + 2, d: i + 3 });
    if (keep.length >= 2048) {
      keep = [];
    }
  }
  return keep;
}

function main(): void {
  let bad = 0;
  let sameOk = 0;
  const rounds = 24;
  for (let r = 0; r < rounds; r++) {
    const holder = new Holder();
    const reg = new Registry();
    // Fill the initial capacity so the chained set's first insert grows.
    for (let i = 0; i < 8; i++) {
      reg.set("p" + r + "_" + i, i);
    }
    holder.m = reg;
    // Sweep the fill so successive rounds land the grow at different points of
    // the nursery budget.
    const keep = fill(120000 + r * 20000);
    const size = holder.put("a" + r, "b" + r);
    const ok =
      size === 10 &&
      holder.m.get("a" + r) === 1 &&
      holder.m.get("b" + r) === 2 &&
      holder.m.get("p" + r + "_7") === 7;
    if (!ok) {
      bad++;
    }
    if (holder.same("s" + r)) {
      sameOk++;
    }
    if (keep.length < 0) {
      console.log("unreachable");
    }
  }
  console.log("map-set-chain bad=" + bad);
  console.log("set returns receiver=" + sameOk + "/" + rounds);
}

main();
