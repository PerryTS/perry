'use strict'
// #10437: CommonJS `require()` outside a function is hoisted and run
// unconditionally at module init, including inside `if (false)` and other
// branches that never run. Every require below except H is inside a branch
// that never executes; only H's side-effect module should ever load, and it
// should load exactly at the point control flow reaches it (between the
// "before taken branch" and "after taken branch" log lines) — not before
// the first statement, and not before H's guarding condition was evaluated.
//
// This is the shape pg 8.22.0 hits verbatim: `lib/index.js` guards an
// optional native binding behind `if (forceNative) { require('./native') }`,
// and `./native` transitively requires the optional, often-uninstalled
// `pg-native`. The crash-form section below reproduces that two-hop shape
// with a target that genuinely does not resolve on disk.

console.log('start')

// A: literal false
if (false) {
  require('./gap10437_side_a.cjs')
}
// B: short-circuit
false && require('./gap10437_side_b.cjs')
// C: runtime-false env check (pg's `if (forceNative)` shape)
if (process.env.PERRY_GAP10437_UNSET_C) {
  require('./gap10437_side_c.cjs')
}
// D: ternary arm not taken
const d = process.env.PERRY_GAP10437_UNSET_D ? require('./gap10437_side_d.cjs') : 'd-skipped'
// E: switch case not taken
switch (1) {
  case 2:
    require('./gap10437_side_e.cjs')
}
// F: loop body never runs
for (let i = 0; i < 0; i++) require('./gap10437_side_f.cjs')
// G: function never called (already correctly deferred pre-#10437)
function never() {
  return require('./gap10437_side_g.cjs')
}

console.log('before taken branch')

// H: the taken branch — must load exactly here, not earlier.
if (true) {
  require('./gap10437_side_h.cjs')
}

console.log('after taken branch, d=' + d)

// Caching: two conditional requires of the SAME module must run the side
// effect once and return the SAME exports object both times.
let capA = null
let capB = null
if (true) {
  capA = require('./gap10437_counter.cjs')
}
if (true) {
  capB = require('./gap10437_counter.cjs')
}
console.log('cache same=' + (capA === capB) + ' n=' + capA.n)

// Crash-form (pg-native shape): an optional native binding behind an unset
// env check, whose target itself unconditionally (but inside a
// non-swallowing try/catch) requires a module that does not exist on disk.
// Pre-fix this crashed the whole program with "Cannot find module" even
// though the guarding env var was never set.
let impl = 'js'
if (process.env.PERRY_GAP10437_USE_NATIVE) {
  impl = require('./gap10437_native_rethrow.cjs')
}
console.log('impl=' + impl)

// A genuinely missing module behind a try/catch that SWALLOWS the error,
// itself nested inside a condition that never runs.
let fallback = 'default'
if (process.env.PERRY_GAP10437_UNSET_FALLBACK) {
  try {
    fallback = require('./gap10437_does_not_exist.cjs')
  } catch (e) {
    fallback = 'caught'
  }
}
console.log('fallback=' + fallback)

console.log('end')

module.exports = { never: never }
