// Issue #314: indirect closure calls were rejected at codegen for any arity > 5
// even though the runtime exposes js_closure_call0..js_closure_call16.
// Effect's data-first `dual()` plus context plus typed-error params drives 6
// arg ("metric/hook", "sink", "stm/tPubSub"), 8 arg ("stm/core"), and 10 arg
// ("schedule") closure calls — the ceiling at 5 blocked five Effect modules.

// Case 1: 6-arg closure-typed param (the issue's minimal repro).
const f6 = (a: number, b: number, c: number, d: number, e: number, fa: number) =>
  a + b + c + d + e + fa;
const g6 = (
  h: (a: number, b: number, c: number, d: number, e: number, fa: number) => number,
) => h(1, 2, 3, 4, 5, 6);
console.log("case1:", g6(f6));

// Case 2: 8-arg (matches `effect/src/internal/stm/core.ts` func 13).
const f8 = (
  a: number, b: number, c: number, d: number,
  e: number, fa: number, g: number, h: number,
) => a + b + c + d + e + fa + g + h;
const g8 = (
  cb: (a: number, b: number, c: number, d: number, e: number, fa: number, g: number, h: number) => number,
) => cb(1, 2, 3, 4, 5, 6, 7, 8);
console.log("case2:", g8(f8));

// Case 3: 10-arg (matches `effect/src/internal/schedule.ts` func 127 — the
// largest in the Effect compat sweep).
const f10 = (
  a: number, b: number, c: number, d: number, e: number,
  fa: number, g: number, h: number, i: number, j: number,
) => a + b + c + d + e + fa + g + h + i + j;
const g10 = (
  cb: (
    a: number, b: number, c: number, d: number, e: number,
    fa: number, g: number, h: number, i: number, j: number,
  ) => number,
) => cb(1, 2, 3, 4, 5, 6, 7, 8, 9, 10);
console.log("case3:", g10(f10));

// Case 4: 5-arg still works (regression guard — the previous ceiling).
const f5 = (a: number, b: number, c: number, d: number, e: number) =>
  a + b + c + d + e;
const g5 = (
  cb: (a: number, b: number, c: number, d: number, e: number) => number,
) => cb(1, 2, 3, 4, 5);
console.log("case4:", g5(f5));

// Case 5: 16-arg upper bound (matches js_closure_call16, the runtime cap).
const f16 = (
  a: number, b: number, c: number, d: number,
  e: number, fa: number, g: number, h: number,
  i: number, j: number, k: number, l: number,
  m: number, n: number, o: number, p: number,
) => a + b + c + d + e + fa + g + h + i + j + k + l + m + n + o + p;
const g16 = (
  cb: (
    a: number, b: number, c: number, d: number,
    e: number, fa: number, g: number, h: number,
    i: number, j: number, k: number, l: number,
    m: number, n: number, o: number, p: number,
  ) => number,
) => cb(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16);
console.log("case5:", g16(f16));
