// #7963: `Object.defineProperty` itself — the wider window #6949's scope note
// names and defers, and the one #7949 (`Object.defineProperties`) deliberately
// left open.
//
// `js_object_define_property` resolves the receiver's `ObjectHeader` (`obj`)
// and coerces the key to a `StringHeader` (`key_str`) ONCE, near the top, and
// then keeps both as bare Rust locals through the whole rest of the function:
// the descriptor-field reads (`desc_has_field` / `desc_read_field`, each of
// which allocates the field-name string and can run a USER GETTER on the
// descriptor bag), `enforce_define_property_invariants`,
// `ensure_key_in_keys_array` (which grows the keys array), and
// `define_property_force_store_value`. A raw `*mut ObjectHeader` / `*const
// StringHeader` in a Rust local is neither a shadow slot nor a temp root nor
// reachable from any registered scanner, so an evacuating minor landing in any
// of those calls can neither keep them alive nor rewrite them.
//
// The stale receiver address is also the OWNER KEY of the per-property
// descriptor side tables (`set_property_attrs` / `set_accessor_descriptor` /
// `accessor_descriptors`), so a stale `obj` files the attributes under a dead
// address where the matching read can never find them — a silent wrong answer
// rather than a crash.
//
// Why this needs a hand-built probe: `scripts/gc_root_dominance_check.py` reads
// emitted LLVM IR, and a Rust-side local is structurally invisible to it.
//
// LIVE BY CONSTRUCTION, in two ways:
//   * the FIRST arm (`objectGroupBy`) allocates hard enough to fill and retire
//     from-space blocks, so the second arm's stale reads land in RETIRED bytes
//     the quarantine can name rather than in bytes nobody has reused yet;
//   * every descriptor getter runs `churn`, which has a loop back-edge (a GC
//     safepoint poll is emitted only in user JS) and keeps allocating after it.
//
// Witness configuration:
//
//   PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1 \
//   PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800
//
// Before the fix this exits 138 with `[gc-fromspace-protect] FAULT` naming a
// retired from-space address during arm 3 (`obj_type=2`, a receiver
// `ObjectHeader`); after it the program exits 0 with 301 copying minors and
// ~110k objects moved, byte-identical to node in every configuration.

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

// Arm 1 — allocate hard through a user callback so from-space blocks are
// retired before arm 2 runs. (This is `test_gap_gc_container_value_rooting`'s
// `objectGroupBy`; #7949 fixed the container, so this arm is expected to be
// correct on its own. It is here for the allocation profile.)
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

// Arm 2 — a HAND-WRITTEN `Object.defineProperty` loop. `Object.defineProperties`
// (the #7949 helper) is deliberately NOT on this path: the descriptor bag is
// walked here, in JS, and each descriptor is installed one at a time. What is
// left is `js_object_define_property`'s own window.
function definePropertyOneAtATime(): string {
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
  for (const key of Object.keys(bag)) {
    Object.defineProperty(target, key, bag[key]);
  }

  const parts: string[] = [];
  for (const key of Object.keys(target).sort()) {
    parts.push(key + "=" + target[key]);
  }
  return parts.join("|");
}

// Arm 3 — the descriptor bag itself carries ACCESSOR fields whose getters
// allocate, so the descriptor read inside `js_object_define_property` runs user
// JS between the key coercion and the key's use. This is the shape the #6949
// scope note calls out directly ("holds `obj` / `descriptor_value` and the six
// raw `JSValue`s inside `DescView` across its own later `js_string_from_bytes`
// calls").
function definePropertyWithAllocatingDescriptorGetters(): string {
  const target: any = {};
  for (let i = 0; i < 12; i++) {
    const index = i;
    const descriptor: any = {};
    Object.defineProperty(descriptor, "value", {
      enumerable: true,
      get: function (): string {
        churn(index);
        return "v" + index;
      },
    });
    Object.defineProperty(descriptor, "enumerable", {
      enumerable: true,
      get: function (): boolean {
        churn(index);
        return true;
      },
    });
    Object.defineProperty(descriptor, "configurable", {
      enumerable: true,
      get: function (): boolean {
        churn(index);
        return true;
      },
    });
    Object.defineProperty(target, "key-" + index, descriptor);
  }
  const parts: string[] = [];
  for (const key of Object.keys(target).sort()) {
    parts.push(key + "=" + target[key]);
  }
  return parts.join("|");
}

function expectedIndexed(prefix: string, valuePrefix: string, count: number): string {
  const keys: string[] = [];
  for (let i = 0; i < count; i++) {
    keys.push(prefix + i);
  }
  // Sort the KEYS, exactly like `Object.keys(target).sort()` does — sorting the
  // joined "key=value" strings orders "prop-10=" before "prop-1=".
  keys.sort();
  const parts: string[] = [];
  for (const key of keys) {
    parts.push(key + "=" + valuePrefix + key.substring(prefix.length));
  }
  return parts.join("|");
}

// NOTE — why each side is bound to a `const` instead of being compared inline.
//
// `console.log("x", f() === g() ? …)` leaves `f()`'s result as an SSA temporary
// that is live across `g()`. Under this witness configuration `g()` allocates
// through several loop back-edges, so it collects, and the temporary names
// from-space: the run faults inside `js_jsvalue_equals` (frame `js_eq` <- `main`)
// on BOTH a pristine build and this branch. That is a SEPARATE, pre-existing
// codegen root-dominance defect — the class
// `scripts/gc_root_dominance_check.py` exists for — and it has nothing to do
// with `Object.defineProperty`. Binding both sides first keeps this program a
// witness for ONE defect. See the issue filed alongside #7963.
const groupByObserved = objectGroupBy();
const groupByExpected = expectedObjectGroupBy();
console.log("objectGroupBy", groupByObserved === groupByExpected ? "ok" : "BAD");

const oneAtATimeObserved = definePropertyOneAtATime();
const oneAtATimeExpected = expectedIndexed("prop-", "value-", 12);
console.log(
  "definePropertyOneAtATime",
  oneAtATimeObserved === oneAtATimeExpected ? "ok" : "BAD",
);

const accessorObserved = definePropertyWithAllocatingDescriptorGetters();
const accessorExpected = expectedIndexed("key-", "v", 12);
console.log(
  "definePropertyAccessorDescriptor",
  accessorObserved === accessorExpected ? "ok" : "BAD",
);
