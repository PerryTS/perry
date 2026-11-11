import { execFileSync, spawnSync } from "node:child_process";

const size = 256 * 1024;
const code = `process.stdout.write('x'.repeat(${size}));process.stderr.write('y'.repeat(${size}))`;
const spawned = spawnSync("node", ["-e", code], {
  encoding: "utf8",
  maxBuffer: Infinity,
});
console.log("spawnSync status:", spawned.status);
console.log("spawnSync stdout length:", spawned.stdout.length);
console.log("spawnSync stderr length:", spawned.stderr.length);
console.log("spawnSync error:", spawned.error?.code ?? "none");

const output = execFileSync(
  "node",
  ["-e", `process.stdout.write('z'.repeat(${size}))`],
  {
    encoding: "utf8",
    maxBuffer: Infinity,
  },
);
console.log("execFileSync length:", output.length);
