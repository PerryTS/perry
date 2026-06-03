import { execFileSync, execSync } from "node:child_process";

function slot(value: any): string {
  if (value === null) return "null";
  if (Buffer.isBuffer(value)) return `buffer:${value.toString("utf8")}`;
  return `${typeof value}:${String(value)}`;
}

function reportValue(label: string, value: any) {
  console.log(`${label} value:`, slot(value));
}

function reportThrow(label: string, action: () => any) {
  try {
    reportValue(label, action());
  } catch (err: any) {
    console.log(`${label} status:`, err.status);
    console.log(`${label} stdout:`, slot(err.stdout));
    console.log(`${label} stderr:`, slot(err.stderr));
    console.log(`${label} output:`, err.output.map(slot).join("|"));
  }
}

reportValue(
  "execSync all ignore",
  execSync("printf ignored; printf ignored-err >&2", {
    encoding: "utf8",
    stdio: "ignore",
  }),
);

reportValue(
  "execSync all inherit",
  execSync("exit 0", { encoding: "utf8", stdio: "inherit" }),
);

reportThrow("execSync stdout ignore stderr pipe throw", () =>
  execSync("printf out; printf err >&2; exit 7", {
    encoding: "utf8",
    stdio: ["ignore", "ignore", "pipe"],
  }),
);

reportValue(
  "execFileSync all ignore",
  execFileSync("sh", ["-c", "printf ignored; printf ignored-err >&2"], {
    encoding: "utf8",
    stdio: "ignore",
  }),
);

reportThrow("execFileSync stdout pipe stderr ignore throw", () =>
  execFileSync("sh", ["-c", "printf out2; printf err2 >&2; exit 9"], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "ignore"],
  }),
);
