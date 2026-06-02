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
const threadCpuUsage = process.threadCpuUsage;
const availableMemory = process.availableMemory;
const constrainedMemory = process.constrainedMemory;
const resourceUsage = process.resourceUsage;
const getActiveResourcesInfo = process.getActiveResourcesInfo;
console.log("process.cwd:", typeof cwd);
console.log("process.uptime:", typeof uptime);
console.log("process.memoryUsage:", typeof memoryUsage);
console.log("process.nextTick:", typeof nextTick);
console.log("process.threadCpuUsage:", typeof threadCpuUsage);
console.log("process.availableMemory:", typeof availableMemory);
console.log("process.constrainedMemory:", typeof constrainedMemory);
console.log("process.resourceUsage:", typeof resourceUsage);
console.log("process.getActiveResourcesInfo:", typeof getActiveResourcesInfo);

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
console.log("threadCpuUsage().user is number:", typeof threadCpuUsage().user === "number");
console.log("availableMemory() is number:", typeof availableMemory() === "number");
console.log("constrainedMemory() is number:", typeof constrainedMemory() === "number");
console.log("resourceUsage().maxRSS is number:", typeof resourceUsage().maxRSS === "number");
console.log("getActiveResourcesInfo() is array:", Array.isArray(getActiveResourcesInfo()));
console.log("platform() === os.platform():", platform() === os.platform());
