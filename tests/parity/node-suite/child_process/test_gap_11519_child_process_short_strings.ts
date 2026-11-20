// #11519: child_process commands passed as a SHORT (SSO, <= 5 bytes, built at
// runtime) string. `execSync` / `spawnSync` / `exec` / `spawn` unboxed the
// command with a bare mask, so `execSync("ec" + "ho")` segfaulted.
import { execSync, spawnSync, exec, spawn } from "node:child_process";
const S = (s: string): string => s.charAt(0) + s.slice(1);

console.log("execSync:", JSON.stringify(execSync(S("echo")).toString()), JSON.stringify(execSync(S("true")).toString()));
console.log("execSync tpl:", JSON.stringify(execSync(`${"ec"}${"ho"}`, { encoding: "utf8" })));
const r = spawnSync(S("echo"), [S("a"), String(12)], { encoding: "utf8" });
console.log("spawnSync:", r.status, JSON.stringify(r.stdout));
const t = spawnSync(S("true"));
console.log("spawnSync true:", t.status);
exec(S("echo"), (err, stdout) => {
  console.log("exec:", err === null, JSON.stringify(stdout));
  const child = spawn(S("echo"), [S("hi")]);
  let out = "";
  child.stdout.on("data", (d: any) => (out += d));
  child.on("close", (code: number) => console.log("spawn:", code, JSON.stringify(out)));
});
