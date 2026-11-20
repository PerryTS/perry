// Refs #2135 (node:process stubbed methods): `process.getgroups()` is a
// POSIX accessor that returns the supplementary group IDs for the
// current process. Previously it read back as a 0 (typeof `"number"`),
// so duck-type guards (`typeof process.getgroups === "function"`) saw
// the stub and downstream code that called the value never ran.
//
// The runtime now wraps `libc::getgroups(2)`. The HIR lowers the static
// `process.getgroups()` call through the generic NativeMethodCall path
// which routes to `js_process_getgroups` via the node_core table; the
// property-read form returns a bound-method closure (`typeof "function"`,
// matching Node) via the existing process-callable-export whitelist.

console.log(typeof process.getgroups);
console.log(Array.isArray(process.getgroups()));
console.log(process.getgroups().length >= 1);
console.log(typeof process.getgroups()[0]);

// The set we return matches the OS `id -G` output (every entry is a
// non-negative integer GID). Sanity-check the shape without pinning a
// host-specific count.
const groups = process.getgroups();
console.log(groups.every((g: number) => Number.isInteger(g) && g >= 0));
