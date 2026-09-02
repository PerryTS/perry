// #9485: a METHOD CALL on the CJS-default namespace object that
// `require('child_process')` (and `process.getBuiltinModule('child_process')`)
// returns dispatched under the module name `"child_process.default"`. The
// runtime's method-call router normalized only a hand-maintained subset of the
// `<mod>.default` names — `child_process.default` was not on it — so the call
// found no dispatch bucket and returned `undefined` WITHOUT SPAWNING ANYTHING.
//
// The asymmetry is the tell: the two-step forms (`const f = cp.spawn; f(...)`,
// `cp.spawn.call(...)`) always worked, because the property-READ path resolves
// through the canonical `cjs_default_base_module` table. Only the fused
// member-call form was broken.
//
// That is exactly the shape `cross-spawn` uses
// (`const cp = require('child_process'); cp.spawn(command, args, options)`),
// which is what the MCP SDK's StdioClientTransport spawns servers with — so
// claude-code under perry reported `✗ Failed to connect` for every stdio MCP
// server while never launching a child at all.
import { createRequire } from "node:module";

const req = createRequire(import.meta.url);

const cp: any = req("child_process");
const dns: any = req("dns");
const os: any = req("os");

// ── 1. the regression: fused member call on the CJS-default namespace ──
const sync = cp.spawnSync("/bin/echo", ["spawnSync-member"], { encoding: "utf8" });
console.log("spawnSync member typeof:", typeof sync);
console.log("spawnSync member stdout:", String(sync.stdout).trim());

// A computed member call takes the same route.
const syncComputed = cp["spawnSync"]("/bin/echo", ["spawnSync-computed"], { encoding: "utf8" });
console.log("spawnSync computed stdout:", String(syncComputed.stdout).trim());

console.log("execSync member stdout:", String(cp.execSync("/bin/echo execSync-member")).trim());

// ── 2. control: the two-step forms were never broken ──
const hoisted = cp.spawnSync;
console.log("spawnSync hoisted stdout:", String(hoisted("/bin/echo", ["hoisted"], { encoding: "utf8" }).stdout).trim());

// ── 3. sibling `<mod>.default` namespaces that shared the gap ──
console.log("dns.getServers isArray:", Array.isArray(dns.getServers()));

// ── 4. a namespace that was already on the list, as a negative control ──
console.log("os.platform is string:", typeof os.platform() === "string");

// ── 5. the streaming spawn the MCP client actually uses ──
const child = cp.spawn("/bin/echo", ["spawn-streams"]);
console.log("spawn member typeof:", typeof child);
console.log("spawn member pid is number:", typeof child.pid === "number");
await new Promise<void>((resolve) => {
  let buf = "";
  child.stdout.on("data", (chunk: any) => {
    buf += chunk.toString();
  });
  child.on("exit", () => {
    console.log("spawn member stdout:", buf.trim());
    resolve();
  });
});
