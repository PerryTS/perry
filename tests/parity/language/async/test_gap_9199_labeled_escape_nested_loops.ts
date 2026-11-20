// #9199: a labeled `break`/`continue` that targets an OUTER loop from inside a
// NESTED loop, in a function the generator linearizer rewrites (sync generator,
// async generator, async function). The linearizer converted labeled
// completions only at the labeled loop's own body level and stopped at nested
// loops, so an escape that crossed a loop boundary survived into a state body
// and the dispatch lowering dropped it: `break` produced a malformed iterator
// result ("Cannot read properties of undefined (reading 'done')") and
// `continue` silently produced nothing at all.
//
// Rows that already worked before the fix are kept: the switch-wrapped forms
// took a different route (#9186/#9189) and must not regress.

async function drain(label: string, mk: () => AsyncIterable<string>): Promise<void> {
  const acc: string[] = [];
  try {
    for await (const v of mk()) acc.push(v);
    console.log(label + ": " + acc.join(","));
  } catch (e) {
    console.log(label + ": THREW " + String((e as Error) && (e as Error).message));
  }
}
function drainSync(label: string, it: Iterable<string>): void {
  const acc: string[] = [];
  try {
    for (const v of it) acc.push(v);
    console.log(label + ": " + acc.join(","));
  } catch (e) {
    console.log(label + ": THREW " + String((e as Error) && (e as Error).message));
  }
}

// ── async generators ──────────────────────────────────────────────────────
async function* agBreakOuter() {
  O: for (const x of [1, 2]) { I: for (const y of [0, 1]) { yield "b" + x + y; break O; } }
}
async function* agContinueOuter() {
  O: for (const x of [1, 2]) { I: for (const y of [0, 1]) { yield "c" + x + y; continue O; } }
}
async function* agBreakOuterAwait() {
  O: for (const x of [1, 2]) { I: for (const y of [0, 1]) { await Promise.resolve(); yield "w" + x + y; break O; } }
}
async function* agThreeDeep() {
  A: for (const x of [1, 2]) { B: for (const y of [0, 1]) { C: for (const z of [0, 1]) {
    yield "t" + x + y + z; if (y === 1) break A; continue B; } } }
}
async function* agConditional() {
  O: for (const x of [1, 2, 3]) { I: for (const y of [0, 1]) {
    yield "n" + x + y; if (x === 2) break O; if (y === 1) continue O; } }
}
async function* agSwitchBreakOuter() {
  O: for (const x of [1, 2]) { I: for (const y of [0, 1]) {
    switch (x + y) { case 3: break O; case 1: yield "s" + x + y; break; default: break I; } } }
}
async function* agTryFinally() {
  const seen: string[] = [];
  O: for (const x of [1, 2]) { I: for (const y of [0, 1]) {
    try { yield "f" + x + y; if (y === 0) continue O; break O; } finally { seen.push("fin" + x + y); } } }
  yield "seen=" + seen.join("/");
}
// A sibling loop reusing a label name after the first one closed: legal, and
// the escape must bind to the loop that is actually enclosing.
async function* agReusedLabel() {
  L: for (const x of [1, 2]) { I: for (const y of [0, 1]) { yield "h" + x + y; break L; } }
  L: for (const x of [3, 4]) { I: for (const y of [0, 1]) { yield "h" + x + y; continue L; } }
}
async function* agWhileOuter() {
  let i = 0;
  O: while (i < 2) { i++; I: for (const y of [0, 1]) { yield "l" + i + y; continue O; } }
}

// ── sync generators ───────────────────────────────────────────────────────
function* sgBreakOuter() {
  O: for (const x of [1, 2]) { I: for (const y of [0, 1]) { yield "B" + x + y; break O; } }
}
function* sgContinueOuter() {
  O: for (const x of [1, 2]) { I: for (const y of [0, 1]) { yield "C" + x + y; continue O; } }
}

// ── async functions ───────────────────────────────────────────────────────
async function afBreakOuter(): Promise<string> {
  const a: string[] = [];
  O: for (const x of [1, 2]) { I: for (const y of [0, 1]) { await Promise.resolve(); a.push("F" + x + y); break O; } }
  return a.join(",");
}
async function afContinueOuter(): Promise<string> {
  const a: string[] = [];
  O: for (const x of [1, 2]) { I: for (const y of [0, 1]) { await Promise.resolve(); a.push("G" + x + y); continue O; } }
  return a.join(",");
}

(async () => {
  await drain("ag-break-outer", agBreakOuter);
  await drain("ag-continue-outer", agContinueOuter);
  await drain("ag-break-outer-await", agBreakOuterAwait);
  await drain("ag-three-deep", agThreeDeep);
  await drain("ag-conditional", agConditional);
  await drain("ag-switch-break-outer", agSwitchBreakOuter);
  await drain("ag-try-finally", agTryFinally);
  await drain("ag-reused-label", agReusedLabel);
  await drain("ag-while-outer", agWhileOuter);
  drainSync("sg-break-outer", sgBreakOuter());
  drainSync("sg-continue-outer", sgContinueOuter());
  console.log("af-break-outer: " + (await afBreakOuter()));
  console.log("af-continue-outer: " + (await afContinueOuter()));
  console.log("DONE");
})();
