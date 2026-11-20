import { readFileSync, existsSync } from "node:fs";
import { load } from "./generated/bindings";
const bytes = readFileSync(process.argv[2]);
const root = process.argv[3];
function check(ok: boolean, message: string): void { if (!ok) throw new Error(message); }
let absent = false;
try { load(bytes, {}, { memoryBytes: 268435456, fuel: 100000000 }); } catch (_) { absent = true; }
check(absent, "WASI must be opt-in");
const closed = load(bytes, {}, { wasi: {}, memoryBytes: 268435456, fuel: 100000000 });
const a = load(bytes, {}, { memoryBytes: 268435456, fuel: 100000000, wasi: {
  args: ["first", "alpha"], env: { MARKER: "one" },
  preopens: [{ hostPath: root + "/one", guestPath: "/data" }]
} });
const b = load(bytes, {}, { memoryBytes: 268435456, fuel: 100000000, wasi: {
  args: ["second", "beta"], env: { MARKER: "two" },
  preopens: [{ hostPath: root + "/two", guestPath: "/data", readOnly: false }]
} });
const cp = closed.exports["perry:capabilities/probe@1.0.0"];
const ap = a.exports["perry:capabilities/probe@1.0.0"];
const bp = b.exports["perry:capabilities/probe@1.0.0"];
for (let i = 0; i < 3; i++) {
  // Perry inserts the source entry at argv[1], matching its native Node API.
  const aa = ap.arguments(), ba = bp.arguments();
  check(aa.length === 3 && aa[0] === "first" && aa[2] === "alpha", "args A");
  check(ba.length === 3 && ba[0] === "second" && ba[2] === "beta", "args B");
  const ae = ap.environment("MARKER"), be = bp.environment("MARKER");
  check(ae.tag === "some" && ae.value === "one", "env A");
  check(be.tag === "some" && be.value === "two", "env B");
  const ar = ap.read("/data/input.txt"), br = bp.read("/data/input.txt");
  check(ar.tag === "ok" && ar.value === "one", "file A");
  check(br.tag === "ok" && br.value === "two", "file B");
}
check(cp.arguments().length === 0, "ambient args leaked");
check(ap.environment("PERRY_COMPONENT_AMBIENT").tag === "none", "ambient env A leaked");
check(bp.environment("PERRY_COMPONENT_AMBIENT").tag === "none", "ambient env B leaked");
check(cp.environment("PERRY_COMPONENT_AMBIENT").tag === "none", "ambient env leaked");
check(cp.environment("MARKER").tag === "none", "instance env leaked");
check(cp.read("/data/input.txt").tag === "err", "no-preopen read allowed");
check(cp.write("/data/output.txt", "bad").tag === "err", "no-preopen write allowed");
check(ap.write("/data/output.txt", "bad").tag === "err", "read-only write allowed");
check(ap.read("/data/../outside.txt").tag === "err", "parent escape allowed");
check(ap.read("/data/escape.txt").tag === "err", "symlink escape allowed");
check(bp.write("/data/output.txt", "written").tag === "ok", "explicit write denied");
check(readFileSync(root + "/two/output.txt", "utf8") === "written", "write not visible");
check(!existsSync(root + "/one/output.txt"), "read-only directory changed");
closed.dispose(); a.dispose(); b.dispose();
console.log("PASS WASI providers: opt-in, instance isolation, args, env, read-only, write grants, escape denial");
