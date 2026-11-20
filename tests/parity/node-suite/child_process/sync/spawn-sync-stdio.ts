import { spawnSync } from "node:child_process";

function slot(value: any): string {
  if (value === null) return "null";
  if (Buffer.isBuffer(value)) return `buffer:${value.toString("utf8")}`;
  return `${typeof value}:${String(value)}`;
}

function report(label: string, result: any) {
  console.log(`${label} status:`, result.status);
  console.log(`${label} stdout:`, slot(result.stdout));
  console.log(`${label} stderr:`, slot(result.stderr));
  console.log(`${label} output:`, result.output.map(slot).join("|"));
}

report(
  "default pipe",
  spawnSync("sh", ["-c", "printf out; printf err >&2"], { encoding: "utf8" }),
);

report(
  "all ignore",
  spawnSync("sh", ["-c", "printf ignored; printf ignored-err >&2"], {
    encoding: "utf8",
    stdio: "ignore",
  }),
);

report(
  "all inherit",
  spawnSync("sh", ["-c", "exit 0"], { encoding: "utf8", stdio: "inherit" }),
);

report(
  "stdin pipe outputs ignored",
  spawnSync("cat", [], {
    encoding: "utf8",
    input: "input ignored by stdout",
    stdio: ["pipe", "ignore", "ignore"],
  }),
);

report(
  "stdin ignore outputs pipe",
  spawnSync("sh", ["-c", "printf out2; printf err2 >&2"], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }),
);

report(
  "mixed capture",
  spawnSync("sh", ["-c", "printf out3; printf err3 >&2"], {
    encoding: "utf8",
    stdio: ["ignore", "ignore", "pipe"],
  }),
);

report(
  "null entries",
  spawnSync(
    "node",
    [
      "-e",
      "process.stdout.write('null-out'); process.stderr.write('null-err')",
    ],
    { encoding: "utf8", stdio: [null, null, null] },
  ),
);
