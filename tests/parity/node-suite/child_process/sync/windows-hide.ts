import { execFileSync, spawnSync } from "node:child_process";

for (const value of [undefined, null, false, true] as const) {
  const result = spawnSync("node", ["-e", "process.stdout.write('ok')"], {
    windowsHide: value,
    encoding: "utf8",
  });
  console.log(`spawnSync ${String(value)}:`, result.status, result.stdout);
}

console.log(
  "execFileSync true:",
  execFileSync("node", ["-e", "process.stdout.write('file-ok')"], {
    windowsHide: true,
    encoding: "utf8",
  }),
);
