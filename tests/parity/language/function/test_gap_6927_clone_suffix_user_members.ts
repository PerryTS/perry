// Issue #6927: user members whose names spell a generated clone suffix
// (`__generic`, `__typed_f64`, `__typed_i32`, `__typed_i1`, `__typed_string`,
// `__typed_f64_recv`, `__pshape`, `__spec_*`, `__dupN`) must not collide with
// the compiler's clone symbols. Clone symbols now live in a reserved
// `$`-separated namespace user names cannot compose, so every member below
// keeps its own symbol AND its siblings keep their clones.
//
// Pre-fix witness (v0.5.1280): `deduped_function_refs` (first-define-wins)
// silently dropped the user function's public entry when `add`'s typed clone
// composed the same symbol, so the indirect call below returned 5 instead
// of 6.

// ── Function family ────────────────────────────────────────────────────────
function add(a: number, b: number): number {
  return a + b;
}
function add__typed_f64(a: number, b: number): number {
  return a * b;
}
function add__generic(a: number, b: number): number {
  return a - b;
}
function flag(a: boolean): boolean {
  return !a;
}
function flag__typed_i1(a: boolean): boolean {
  return a;
}
function pick(a: number): number {
  return a | 0;
}
function pick__typed_i32(a: number): number {
  return (a | 0) + 1;
}

// Direct calls (statically provable — routed to typed clones).
console.log(add(2, 3), add__typed_f64(2, 3), add__generic(2, 3));
console.log(flag(true), flag__typed_i1(true));
console.log(pick(7), pick__typed_i32(7));

// Indirect calls through function values (routed through the registered
// public symbols — the shape that silently miscompiled pre-fix).
const fns: Array<(a: number, b: number) => number> = [
  add,
  add__typed_f64,
  add__generic,
];
for (const f of fns) {
  console.log(f(2, 3));
}

// ── Method family ──────────────────────────────────────────────────────────
class C {
  foo(): number {
    return 1;
  }
  foo__typed_f64(): number {
    return 2;
  }
  foo__generic(): number {
    return 3;
  }
  foo__pshape(): number {
    return 4;
  }
}
const c = new C();
console.log(c.foo(), c.foo__typed_f64(), c.foo__generic(), c.foo__pshape());

// Dynamic (computed-name) dispatch through the vtable-registered publics.
const names = ["foo", "foo__typed_f64", "foo__generic", "foo__pshape"] as const;
for (const n of names) {
  console.log((c as any)[n]());
}
