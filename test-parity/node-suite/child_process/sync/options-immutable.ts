import { spawnSync } from "node:child_process";

const options = {
  cwd: process.cwd(),
  encoding: "utf8" as const,
  env: { ...process.env, PERRY_SYNC_IMMUTABLE: "yes" },
  input: "input",
  windowsHide: true,
};
const before = JSON.stringify({
  cwd: options.cwd,
  encoding: options.encoding,
  value: options.env.PERRY_SYNC_IMMUTABLE,
  input: options.input,
  windowsHide: options.windowsHide,
});
const result = spawnSync(
  "node",
  ["-e", "process.stdin.pipe(process.stdout)"],
  options,
);
console.log("status:", result.status);
console.log("stdout:", result.stdout);
console.log(
  "unchanged:",
  JSON.stringify({
    cwd: options.cwd,
    encoding: options.encoding,
    value: options.env.PERRY_SYNC_IMMUTABLE,
    input: options.input,
    windowsHide: options.windowsHide,
  }) === before,
);
