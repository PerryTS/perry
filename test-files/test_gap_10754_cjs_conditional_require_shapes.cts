// #10754: a top-level CommonJS `require()` whose call site sits in a
// short-circuit (`cond && require(...)`), a ternary consequent
// (`cond ? require(...) : x`), a `try` block, a `switch` case or a loop body
// was SILENTLY NEVER EVALUATED — the module did not load even when the
// branch was taken. Node loads it. No crash, no diagnostic: the dependency's
// side effects simply never happened.
//
// History. #10437 was the opposite bug: every such `require()` was hoisted
// into an eager synthetic import and ran even when its branch did not.
// #10674 fixed that by DEFERRING the target instead of hoisting it, and got
// the `if (cond) require(...)` shape right. In the five shapes above the
// deferral had no evaluation site left: the require shim returned the
// deferred import binding, and firing the target's `__init()` was gated on
// that binding being a known imported FUNCTION. A dependency with no
// default export — a side-effect-only module, the commonest shape for a
// polyfill or a registration hook — has no such binding to read, so nothing
// ever ran it.
//
// The fixture therefore asserts BOTH directions for every shape, because a
// fix that makes the module load unconditionally is #10437 again, not a fix:
//
//   on_sfx_*  taken branch, SIDE-EFFECT-ONLY target  -> must load (the bug)
//   on_exp_*  taken branch, value-returning target   -> must load AND the
//                                                       value must be the
//                                                       target's exports
//   off_*     branch not taken                       -> must NOT load
//
// The not-taken direction with VALUE-RETURNING targets is #10437's own
// fixture (test_gap_cjs_conditional_require_deferred.ts); the side-effect-only
// targets here are the half it does not reach.
//
// `.cts`, deliberately: this repo's package is `"type": "module"`, so a plain
// `.ts` runs as an ES module under Node and a bare `require` dies with
// `require is not defined`. The extension makes both engines agree on the
// CommonJS goal — the goal the issue is about, since the reproducer is a
// `.cjs` program entry. See test_gap_9412's header for the full rationale.
// Keep this file free of top-level `import`/`export`.

// Runtime-unknown guards: neither engine can fold these at compile time, so
// the compiler cannot decide the branch statically and must emit whatever it
// would emit for a genuine runtime condition (`pg`'s `if (forceNative)`).
const off = Boolean(process.env.PERRY_GAP10754_UNSET);
const on = !off;

console.log('start');

// ---------------------------------------------------------------- not taken
// Each of these must print NOTHING. A load line here is #10437 again.
if (off) require('./_helpers/gap10754_off_if.cjs');
off && require('./_helpers/gap10754_off_and.cjs');
const offTern = off ? require('./_helpers/gap10754_off_tern.cjs') : 'skipped';
try {
  if (off) require('./_helpers/gap10754_off_try.cjs');
} catch (e) {
  console.log('unexpected off_try throw');
}
switch (off) {
  case true:
    require('./_helpers/gap10754_off_switch.cjs');
    break;
  default:
}
for (let i = 0; off && i < 1; i++) require('./_helpers/gap10754_off_for.cjs');
console.log('not-taken done, offTern=' + offTern);

// ------------------------------------------- taken, side-effect-only target
// Each load line must appear between its own two markers: the module runs at
// the moment control flow reaches the call, not before the file's first
// statement and not after the branch.
console.log('sfx if:');
if (on) require('./_helpers/gap10754_on_sfx_if.cjs');
console.log('sfx and:');
on && require('./_helpers/gap10754_on_sfx_and.cjs');
console.log('sfx tern:');
const sfxTern = on ? require('./_helpers/gap10754_on_sfx_tern.cjs') : 'skipped';
console.log('sfx try:');
try {
  if (on) require('./_helpers/gap10754_on_sfx_try.cjs');
} catch (e) {
  console.log('unexpected on_sfx_try throw');
}
console.log('sfx switch:');
switch (on) {
  case true:
    require('./_helpers/gap10754_on_sfx_switch.cjs');
    break;
  default:
}
console.log('sfx for:');
for (let i = 0; on && i < 1; i++) require('./_helpers/gap10754_on_sfx_for.cjs');
console.log('sfx done, typeof sfxTern=' + typeof sfxTern);

// ---------------------------------------------- taken, value-returning target
// Same six shapes, but the target replaces `module.exports`. Pins that the
// shim hands back the TARGET's exports, not the synthetic import binding or
// an empty object.
console.log('exp if:');
let expIf: unknown = 'unset';
if (on) expIf = require('./_helpers/gap10754_on_exp_if.cjs');
console.log('exp and:');
let expAnd: unknown = 'unset';
on && (expAnd = require('./_helpers/gap10754_on_exp_and.cjs'));
console.log('exp tern:');
const expTern = on ? require('./_helpers/gap10754_on_exp_tern.cjs') : 'unset';
console.log('exp try:');
let expTry: unknown = 'unset';
try {
  if (on) expTry = require('./_helpers/gap10754_on_exp_try.cjs');
} catch (e) {
  console.log('unexpected on_exp_try throw');
}
console.log('exp switch:');
let expSwitch: unknown = 'unset';
switch (on) {
  case true:
    expSwitch = require('./_helpers/gap10754_on_exp_switch.cjs');
    break;
  default:
}
console.log('exp for:');
let expFor: unknown = 'unset';
for (let i = 0; on && i < 1; i++) expFor = require('./_helpers/gap10754_on_exp_for.cjs');
console.log(
  'exp values: ' +
    [expIf, expAnd, expTern, expTry, expSwitch, expFor].join(','),
);

// Require caching: a second conditional require of an ALREADY-loaded target
// must not re-run its body and must hand back the same exports.
console.log('cache:');
let again: unknown = 'unset';
on && (again = require('./_helpers/gap10754_on_exp_and.cjs'));
console.log('cache same=' + (again === expAnd));

console.log('end');
