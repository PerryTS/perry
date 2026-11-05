// Issue #1780 follow-up: child_process now honors the host-portable
// `{ cwd, env, shell }` options (previously the options object was ignored on
// spawn/exec/execFile/spawnSync/execSync), and `removeListener`/`off`/
// `removeAllListeners` actually remove listeners (previously no-ops).
//
// Deterministic POSIX-only probes; byte-for-byte vs `node --experimental-strip-types`.
import * as cp from "node:child_process";

// Collect a child's stdout, resolving once it exits.
function run(file: string, args: string[], opts?: any): Promise<string> {
  return new Promise((resolve) => {
    const c = cp.spawn(file, args, opts);
    let buf = "";
    c.stdout.on("data", (d: any) => {
      buf += d.toString();
    });
    c.on("exit", () => resolve(buf.trim()));
  });
}

// ── spawn { cwd } — child runs in the given directory ──
const spawnCwd = await run("/bin/pwd", [], { cwd: "/tmp" });
const refCwd = cp.execSync("pwd", { cwd: "/tmp" }).toString().trim();
console.log("spawn cwd matches execSync cwd:", spawnCwd === refCwd);

// ── spawn { env } — Node replaces the environment wholesale ──
console.log(
  "spawn env:",
  await run("/usr/bin/printenv", ["PERRY_CP_VAR"], { env: { PERRY_CP_VAR: "spawn-env-ok" } }),
);

// ── spawn { shell: true } — command string runs through the shell ──
console.log("spawn shell:", await run("echo shell-ok", [], { shell: true }));

// ── exec { cwd } ──
console.log(
  "exec cwd:",
  await new Promise<string>((resolve) => {
    cp.exec("pwd", { cwd: "/tmp" }, (_e: any, so: any) => resolve(String(so).trim()));
  }),
);

// ── execFile { env } ──
console.log(
  "execFile env:",
  await new Promise<string>((resolve) => {
    cp.execFile(
      "/usr/bin/printenv",
      ["PERRY_CP_VAR2"],
      { env: { PERRY_CP_VAR2: "execFile-env-ok" } },
      (_e: any, so: any) => resolve(String(so).trim()),
    );
  }),
);

// ── execFileSync { env } ──
console.log(
  "execFileSync env:",
  cp
    .execFileSync("/usr/bin/printenv", ["PERRY_CP_VAR3"], { env: { PERRY_CP_VAR3: "efs-env-ok" } })
    .toString()
    .trim(),
);

// ── spawnSync { env } ──
console.log(
  "spawnSync env:",
  cp
    .spawnSync("/usr/bin/printenv", ["PERRY_CP_VAR4"], { env: { PERRY_CP_VAR4: "ss-env-ok" } })
    .stdout.toString()
    .trim(),
);

// ── removeListener / off — removed listener does not fire ──
{
  const c = cp.spawn("/bin/echo", ["x"]);
  const seen: string[] = [];
  const a = () => seen.push("a");
  const b = () => seen.push("b");
  c.on("probe", a);
  c.on("probe", b);
  c.removeListener("probe", a);
  c.emit("probe");
  console.log("removeListener:", seen.join(","));
}

// ── removeAllListeners(event) — clears one event ──
{
  const c = cp.spawn("/bin/echo", ["y"]);
  const seen: string[] = [];
  c.on("probe", () => seen.push("z"));
  c.removeAllListeners("probe");
  c.emit("probe");
  console.log("removeAllListeners(event) count:", seen.length);
}

// ── removeAllListeners() — clears every event ──
{
  const c = cp.spawn("/bin/echo", ["w"]);
  const seen: number[] = [];
  c.on("p", () => seen.push(1));
  c.on("q", () => seen.push(2));
  c.removeAllListeners();
  c.emit("p");
  c.emit("q");
  console.log("removeAllListeners() count:", seen.length);
}
