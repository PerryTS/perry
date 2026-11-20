import { execFileSync, execSync } from "node:child_process";

function text(value: unknown) {
  return String(value);
}

function report(label: string, run: () => unknown) {
  try {
    const result = run();
    console.log(`${label} no throw:`, text(result));
  } catch (e: any) {
    console.log(`${label} caught:`, e instanceof Error);
    console.log(`${label} keys:`, Object.keys(e).join(","));
    console.log(`${label} pid type:`, typeof e.pid);
    console.log(`${label} status:`, e.status);
    console.log(`${label} signal:`, e.signal);
    console.log(`${label} stdout:`, text(e.stdout));
    console.log(`${label} stderr:`, text(e.stderr));
    const output = e.output
      .map((x: any) => x === null ? "null" : text(x))
      .join("|");
    console.log(`${label} output:`, output);
  }
}

console.log("execSync success:", text(execSync("printf ok")));
console.log(
  "execFileSync success:",
  text(execFileSync("sh", ["-c", "printf file-ok"])),
);

report("execSync", () =>
  execSync("printf out; printf err >&2; exit 4", { stdio: "pipe" })
);
report("execFileSync", () =>
  execFileSync(
    "sh",
    [
      "-c",
      "printf \"$1\"; printf \"$2\" >&2; exit \"$3\"",
      "sh",
      "arg-out",
      "arg-err",
      "7",
    ],
    { stdio: "pipe" },
  )
);
