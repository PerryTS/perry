import { spawnSync } from "node:child_process";

const argument = "a&b|c<d>e;$HOME`value`*?";
const program = "process.stdout.write(JSON.stringify(process.argv.slice(1)))";

for (const [label, options] of [
  ["default", { encoding: "utf8" }],
  ["shell false", { encoding: "utf8", shell: false }],
] as const) {
  const result = spawnSync(
    "node",
    ["-e", program, argument, "space value"],
    options,
  );
  console.log(`${label} status:`, result.status);
  console.log(`${label} signal:`, result.signal);
  console.log(`${label} error:`, result.error?.code ?? "none");
  console.log(`${label} stdout:`, result.stdout);
  console.log(`${label} stderr:`, JSON.stringify(result.stderr));
}
