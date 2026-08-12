// #7949: JS values retained in ordinary Rust containers across allocating calls.
//
// `Object.groupBy` / `Map.groupBy` accumulate every `(key, item)` pair into a
// `Vec<(f64, f64)>` while calling a USER CALLBACK once per element, and
// `Object.defineProperties` accumulates the descriptor bag's own keys into a
// `Vec<f64>` and then walks them across a `[[Get]]` that can run a user getter.
// A `Vec` on the Rust heap is neither a shadow slot nor a temp root nor
// reachable from any registered scanner, so an evacuating minor landing inside
// one of those callbacks can neither keep the already-collected values alive
// nor rewrite their addresses.
//
// Why this class needs a hand-built probe: `scripts/gc_root_dominance_check.py`
// reads emitted LLVM IR, and a Rust-side container is structurally invisible to
// it. Nothing faults at the collection either — the nursery recycles the bytes
// and the stale word reads a valid but unrelated object, so the failure surfaces
// cycles later as wrong data or `TypeError: … is not a function`.
//
// LIVE BY CONSTRUCTION. Each callback runs `churn`, which (a) contains a loop
// back-edge, so a GC safepoint poll is emitted inside user JS — the only place
// polls fire — and (b) keeps allocating AFTER that poll, so the retired
// from-space bytes are recycled before the runtime reads its accumulator again.
// A stale element therefore returns wrong text rather than the right answer out
// of memory nobody has reused yet.
//
// Run the vulnerable window with `PERRY_GC_SCHEDULE_RATE=1`, which collects at
// every safepoint, so the first back-edge poll inside the first callback
// already evacuates.

function churn(n: number): number {
  const bits: any[] = [];
  for (let i = 0; i < 120; i++) {
    bits.push({ i: i, s: "y" + i, pad: [i, i + 1, i + 2] });
  }
  return bits.length === 120 ? n : -1;
}

function items(count: number): string[] {
  const out: string[] = [];
  for (let i = 0; i < count; i++) {
    out.push("item-" + i);
  }
  return out;
}

// Object.groupBy: string keys. The items are freshly allocated strings (always
// young, so every evacuating minor moves them) and the callback allocates.
function objectGroupBy(): string {
  const grouped = Object.groupBy(items(18), (s: string, i: number): string => {
    churn(i);
    return "bucket-" + (i % 3);
  });
  const parts: string[] = [];
  for (const key of Object.keys(grouped).sort()) {
    parts.push(key + "=" + (grouped as any)[key].join(","));
  }
  return parts.join("|");
}

// Map.groupBy: no key coercion at all, so the KEYS are retained as raw values
// in the same container alongside the items.
function mapGroupBy(): string {
  const grouped = Map.groupBy(items(18), (s: string, i: number): string => {
    churn(i);
    return "k" + (i % 4);
  });
  const keys: string[] = [];
  for (const key of grouped.keys()) {
    keys.push(key);
  }
  keys.sort();
  const parts: string[] = [];
  for (const key of keys) {
    parts.push(key + "=" + (grouped.get(key) as string[]).join(","));
  }
  return parts.join("|");
}

// Object.defineProperties: the own-key list is collected first, then each key
// spans a `[[Get]]` on the properties bag (a user getter here, so a safepoint
// poll lands inside it) plus the define itself.
function definePropertiesWithGetters(): string {
  const bag: any = {};
  for (let i = 0; i < 12; i++) {
    const index = i;
    Object.defineProperty(bag, "prop-" + index, {
      enumerable: true,
      configurable: true,
      get: function (): any {
        churn(index);
        return { value: "value-" + index, enumerable: true, configurable: true };
      },
    });
  }
  const target: any = {};
  Object.defineProperties(target, bag);
  const parts: string[] = [];
  for (const key of Object.keys(target).sort()) {
    parts.push(key + "=" + target[key]);
  }
  return parts.join("|");
}

function expectedObjectGroupBy(): string {
  const buckets: string[][] = [[], [], []];
  for (let i = 0; i < 18; i++) {
    buckets[i % 3].push("item-" + i);
  }
  const parts: string[] = [];
  for (let b = 0; b < 3; b++) {
    parts.push("bucket-" + b + "=" + buckets[b].join(","));
  }
  return parts.join("|");
}

function expectedMapGroupBy(): string {
  const buckets: string[][] = [[], [], [], []];
  for (let i = 0; i < 18; i++) {
    buckets[i % 4].push("item-" + i);
  }
  const parts: string[] = [];
  for (let b = 0; b < 4; b++) {
    parts.push("k" + b + "=" + buckets[b].join(","));
  }
  return parts.join("|");
}

function expectedDefineProperties(): string {
  const parts: string[] = [];
  for (let i = 0; i < 12; i++) {
    parts.push("prop-" + i + "=value-" + i);
  }
  parts.sort();
  return parts.join("|");
}

console.log("objectGroupBy", objectGroupBy() === expectedObjectGroupBy() ? "ok" : "BAD");
console.log("mapGroupBy", mapGroupBy() === expectedMapGroupBy() ? "ok" : "BAD");
console.log(
  "defineProperties",
  definePropertiesWithGetters() === expectedDefineProperties() ? "ok" : "BAD",
);
