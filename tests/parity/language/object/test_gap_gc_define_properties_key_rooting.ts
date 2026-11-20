// #7949: `Object.defineProperties` collected the descriptor bag's own names
// into a plain `Vec<f64>` and then walked that list across, per key:
//
//   * `str_from_value` -> `js_string_coerce`, which allocates for every key
//     shape except an already-heap string;
//   * `js_dynamic_object_get_property`, which runs a USER GETTER on the
//     properties bag;
//   * `js_object_define_property`, which grows the target.
//
// The receiver, the properties bag, the own-names array and the coerced key
// string were bare Rust locals across the same window. A `Vec` on the Rust heap
// is neither a shadow slot nor a temp root nor reachable from any registered
// scanner, so an evacuating minor can neither keep those strings alive nor
// rewrite their addresses — and `scripts/gc_root_dominance_check.py` reads
// emitted LLVM IR, so it cannot see a Rust-side container at all.
//
// LIVE BY CONSTRUCTION. Every getter runs `churn`, which (a) has a loop
// back-edge, so a GC safepoint poll is emitted inside user JS — the only place
// polls fire — and (b) keeps allocating after that poll, so the retired
// from-space bytes are recycled before `defineProperties` reads its key list
// again.
//
// Witness configuration (both needed; the second is what makes it precise):
//
//   PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1 \
//   PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800
//
// Before the fix that faults with `[gc-fromspace-protect] FAULT` naming a
// retired from-space address; after it, this program is byte-identical to node
// in every configuration.

function churn(n: number): number {
  const bits: any[] = [];
  for (let i = 0; i < 120; i++) {
    bits.push({ i: i, s: "y" + i, pad: [i, i + 1, i + 2] });
  }
  return bits.length === 120 ? n : -1;
}

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

function expectedDefineProperties(): string {
  // Sort the KEYS, exactly like `Object.keys(target).sort()` does — sorting the
  // joined "key=value" strings orders "prop-10=" before "prop-1=" and would
  // disagree with the observed side for a reason that has nothing to do with GC.
  const keys: string[] = [];
  for (let i = 0; i < 12; i++) {
    keys.push("prop-" + i);
  }
  keys.sort();
  const parts: string[] = [];
  for (const key of keys) {
    parts.push(key + "=value-" + key.substring(5));
  }
  return parts.join("|");
}

console.log(
  "defineProperties",
  definePropertiesWithGetters() === expectedDefineProperties() ? "ok" : "BAD",
);

// WHY THIS IS ITS OWN PROGRAM, and not another arm of
// `test_gap_gc_container_value_rooting.ts`:
//
// Running a `groupBy` arm first and this one second faults under the witness
// configuration above even WITH #7949 fixed — and it faults identically when
// `Object.defineProperties` is replaced by a hand-written
// `Object.defineProperty` loop, i.e. with none of the code #7949 touches on the
// path. That is a separate, pre-existing rooting defect in the
// `defineProperty`/getter family (the wider window #6949's scope note names and
// defers: `js_object_define_property` holds `obj` / `descriptor_value` and the
// six raw `JSValue`s inside `DescView` across its own later
// `js_string_from_bytes` calls). Keeping the two programs apart means each gap
// test fails for its own reason.
