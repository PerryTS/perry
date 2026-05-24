// #1576: a native-module method read into a variable (a value-position
// read, not a direct `typeof X.m`) must still report typeof "function"
// and stay callable — it routes through js_native_module_property_by_name,
// which returns a bound-method closure for allowlisted (module, method)
// pairs. Captured process methods are also invoked here.
import * as crypto from "node:crypto";
import * as os from "node:os";

// ── captured method values report typeof "function" ──
const cwd = process.cwd;
const uptime = process.uptime;
const memoryUsage = process.memoryUsage;
const nextTick = process.nextTick;
console.log("process.cwd:", typeof cwd);
console.log("process.uptime:", typeof uptime);
console.log("process.memoryUsage:", typeof memoryUsage);
console.log("process.nextTick:", typeof nextTick);

const createHash = crypto.createHash;
const randomUUID = crypto.randomUUID;
const randomBytes = crypto.randomBytes;
const createHmac = crypto.createHmac;
console.log("crypto.createHash:", typeof createHash);
console.log("crypto.randomUUID:", typeof randomUUID);
console.log("crypto.randomBytes:", typeof randomBytes);
console.log("crypto.createHmac:", typeof createHmac);

const platform = os.platform;
const homedir = os.homedir;
console.log("os.platform:", typeof platform);
console.log("os.homedir:", typeof homedir);

// ── namespace import shapes ──
console.log("typeof crypto:", typeof crypto);
console.log("typeof os:", typeof os);

// ── captured-then-called (process) ──
console.log("cwd() === process.cwd():", cwd() === process.cwd());
console.log("uptime() is number:", typeof uptime() === "number");
console.log("memoryUsage().rss is number:", typeof memoryUsage().rss === "number");
console.log("platform() === os.platform():", platform() === os.platform());
