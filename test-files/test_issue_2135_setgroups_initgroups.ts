// Refs #2135 (node:process stubbed methods): `process.setgroups` and
// `process.initgroups` previously read back as `0` (typeof `"number"`),
// so duck-type guards (`typeof process.setgroups === "function"`) saw
// the stub. Both are now wired to libc as no-op-on-bad-input wrappers.
//
// The non-root call form throws `EPERM` in Node by surfacing the libc
// error; Perry's wrapper currently silently drops errors (matching the
// other `set*id` family it sits next to), so the gap test only pins the
// `typeof` shape — exercising the call form here would diverge from
// Node and isn't the surface the original #2135 entry was tracking.

console.log(typeof process.setgroups);
console.log(typeof process.initgroups);
