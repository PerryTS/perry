// Refs #2135 (node:process stubbed methods): the POSIX credential
// setters `process.setuid` / `process.seteuid` / `process.setgid` /
// `process.setegid` previously read back as `0` (typeof `"number"`),
// so a duck-type guard (`typeof process.setuid === "function"`) saw
// the stub and downstream code never invoked them.
//
// The runtime now wraps the matching `libc::set*id(2)` calls. The HIR
// lowers the static call form through `NativeMethodCall`, which routes
// to the new `js_process_set{uid,euid,gid,egid}` functions via the
// node_core dispatch table; the property-read form returns a bound
// closure (`typeof "function"`) via the existing process-callable-export
// whitelist.

console.log(typeof process.setuid);
console.log(typeof process.seteuid);
console.log(typeof process.setgid);
console.log(typeof process.setegid);

// No-op call shape: setting the current id is always permitted, so
// we can exercise the dispatch path without elevating privileges.
const uid = process.getuid!();
const gid = process.getgid!();
process.setuid!(uid);
process.seteuid!(uid);
process.setgid!(gid);
process.setegid!(gid);
console.log("ok");
