// #7770: group-wide numeric-field proof for element-shape groups
// (collectors/ptr_shape_numeric.rs::prove_group_numeric_fields).
//
// A `Ptr<Shape>`-promoted `const r = a[i]` binding now claims numeric fields
// when the WHOLE group proves them: the meet over every push's `new`
// arguments, plus every member's stores. A proven field's number-context
// read is a bare raw load — no `js_number_coerce`, no value check — so the
// direction this can be quietly wrong in is a non-number reaching a claimed
// slot. Every case below routes a non-number through one of the reachable
// store channels and must be BYTE-EXACT against Node.
//
// The promotions and claims themselves are asserted structurally in
// `collectors/ptr_shape_elements_tests.rs` and
// `collectors/ptr_shape_group_numeric_tests.rs`; the zero-coercion IR is
// asserted by the #7770 acceptance run. A green run here with zero
// promotions would be vacuous (#7024/#7025) — this file only pins Node
// equivalence.

class P {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

// 1. The issue's reproducer: every store numeric by construction (loop
//    counter args), fields claimed, reads coercion-free.
function run(n: number): number {
  const a: P[] = [];
  for (let i = 0; i < n; i++) a.push(new P(i, i + 1));
  let s = 0;
  for (let i = 0; i < a.length; i++) {
    const r = a[i];
    s += r.x + r.y;
  }
  return s;
}
console.log("repro:", run(1000));

// 2. A sibling member stores a STRING into a claimed field. The group meet
//    must drop `x` (the read below re-checks), while `y` stays claimed.
function siblingString(): string {
  const a: P[] = [];
  for (let i = 0; i < 3; i++) a.push(new P(i, i * 10));
  for (let i = 0; i < a.length; i++) {
    const w = a[i];
    if (i === 1) (w as any).x = "poison";
  }
  let out = "";
  for (let i = 0; i < a.length; i++) {
    const r = a[i];
    // No `+` on the poisoned field: `o.x + 1` on a string-holding
    // declared-number field is a PRE-EXISTING divergence (numeric-classified
    // Add; reproduces with PERRY_PTR_SHAPE_LOCALS=0 and no arrays — filed
    // separately). Value-context reads pin what #7770 must not break.
    out += `${r.x}|${typeof r.x}|${r.y + 1};`;
  }
  return out;
}
console.log("sibling-string:", siblingString());

// 3. Criterion-5 sweep: null / plain object / BigInt / boolean through a
//    member, read back in value context (typeof, ===) and — where it cannot
//    throw — number context.
function adversarial(): string {
  const a: P[] = [];
  for (let i = 0; i < 4; i++) a.push(new P(i, i));
  for (let i = 0; i < a.length; i++) {
    const w = a[i];
    if (i === 0) (w as any).x = null;
    if (i === 1) (w as any).x = { v: 7 };
    if (i === 2) (w as any).x = 1n;
    if (i === 3) (w as any).x = true;
  }
  let out = "";
  for (let i = 0; i < a.length; i++) {
    const r = a[i];
    out += `${typeof r.x}:${String(r.x)}:${r.x === null}:${(r.x as any) === 1n};`;
    out += `y=${r.y + 1};`;
  }
  return out;
}
console.log("adversarial:", adversarial());

// 4. Number-context read of the null-stored field: ToNumber(null) is 0, and
//    a bare raw load of NaN-boxed null bits would be NaN — the exact
//    divergence the dropped claim must prevent.
function nullNumberContext(): number {
  const a: P[] = [];
  a.push(new P(5, 6));
  for (let i = 0; i < a.length; i++) {
    const w = a[i];
    (w as any).x = null;
  }
  let s = 0;
  for (let i = 0; i < a.length; i++) {
    const r = a[i];
    s += (r.x as any) + 1;
  }
  return s;
}
console.log("null-number-context:", nullNumberContext());

// 5. A method-mediated store with a non-number argument: the parameter
//    resolves through the merged call sites, so `x` drops group-wide.
class Q {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
  setX(v: number): void {
    this.x = v;
  }
}
function methodStore(): string {
  const a: Q[] = [];
  for (let i = 0; i < 3; i++) a.push(new Q(i, i));
  for (let i = 0; i < a.length; i++) {
    const w = a[i];
    if (i === 2) w.setX("s" as any);
    else w.setX(i * 2);
  }
  let out = "";
  for (let i = 0; i < a.length; i++) {
    const r = a[i];
    // No `+` on the poisoned field — same pre-existing divergence as case 2.
    out += `${r.x}|${typeof r.x}|${r.y};`;
  }
  return out;
}
console.log("method-store:", methodStore());

// 6. Mixed push sites: the meet over ALL provenance `new`s — one site passes
//    a string for `x`, so `x` drops even though the other sites are numeric.
function mixedPushSites(): string {
  const a: P[] = [];
  a.push(new P(1, 2));
  a.push(new P("s" as any, 3));
  a.push(new P(4, 5));
  let out = "";
  for (let i = 0; i < a.length; i++) {
    const r = a[i];
    out += `${r.x}|${typeof r.x}|${r.y + 1};`;
  }
  return out;
}
console.log("mixed-push:", mixedPushSites());

// 7. NaN / Infinity / -0 are NUMBERS: the claim survives them, and the bare
//    raw load must reproduce them exactly (including -0 identity).
function specialNumbers(): string {
  const a: P[] = [];
  a.push(new P(NaN, Infinity));
  a.push(new P(-0, -Infinity));
  for (let i = 0; i < a.length; i++) {
    const w = a[i];
    w.y = w.y / 2;
  }
  let out = "";
  for (let i = 0; i < a.length; i++) {
    const r = a[i];
    out += `${r.x}|${r.y}|${Object.is(r.x, -0)}|${r.x === 0};`;
  }
  return out;
}
console.log("special-numbers:", specialNumbers());

// 8. Producer-local pushes plus post-push mutation through the producer —
//    the store is a member store, part of the same group universe.
function producerMix(n: number): number {
  const a: P[] = [];
  for (let i = 0; i < n; i++) {
    const p = new P(i, 0);
    a.push(p);
    p.y = i * 0.5;
  }
  let s = 0;
  for (let i = 0; i < a.length; i++) {
    const r = a[i];
    s += r.x - r.y;
  }
  return s;
}
console.log("producer-mix:", producerMix(100));

// 9. `r.x++` through a member (the member_update fast path consumes the
//    claim): sequence and final values must match Node.
function updateThroughMember(): string {
  const a: P[] = [];
  for (let i = 0; i < 3; i++) a.push(new P(i, 0));
  let seen = "";
  for (let i = 0; i < a.length; i++) {
    const r = a[i];
    seen += `${r.x++},${r.x};`;
  }
  for (let i = 0; i < a.length; i++) {
    const r = a[i];
    seen += `${r.x}`;
  }
  return seen;
}
console.log("update:", updateThroughMember());

// 10. Group integrity: an undeclared-property store on one member voids the
//     whole group (no claims, no promotion) — behavior must be unchanged.
function undeclaredProp(): string {
  const a: P[] = [];
  a.push(new P(1, 2));
  for (let i = 0; i < a.length; i++) {
    const w = a[i];
    (w as any).extra = "e";
  }
  let out = "";
  for (let i = 0; i < a.length; i++) {
    const r = a[i];
    out += `${r.x + 1}|${(r as any).extra};`;
  }
  return out;
}
console.log("undeclared-prop:", undeclaredProp());
