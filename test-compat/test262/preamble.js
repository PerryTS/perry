// Minimal host shims for running Test262 cases outside a dedicated harness
// host (#799). Prepended (after sta.js / assert.js) to every assembled,
// non-`raw` case under BOTH Node and Perry, so the differential still compares
// the two runtimes' builtins — never these shims.
//
// Test262's harness assumes a host that provides a couple of intrinsics that
// neither a bare `node file.js` nor a bare Perry binary defines:
//
//   * print()          — async cases (doneprintHandle.js) report via print().
//   * $DONOTEVALUATE()  — negative *parse* cases call it on the first line as a
//                         tripwire; if the bad code somehow parses, this throws
//                         instead of silently passing.
//
// Both are installed idempotently on globalThis so a test that defines its own
// (or a host that already has one) wins. Test262Error comes from sta.js, which
// is concatenated ahead of this file.

globalThis.print =
  globalThis.print ||
  function (s) {
    console.log(String(s));
  };

globalThis.$DONOTEVALUATE =
  globalThis.$DONOTEVALUATE ||
  function () {
    throw new Test262Error("Test262: This statement should not be evaluated.");
  };
