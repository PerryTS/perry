import { exec, execFile } from "node:child_process";

function valueText(value: any): string {
  return value === null ? "null" : value === undefined ? "undefined" : String(value);
}

function report(label: string, err: any, stdout: unknown, stderr: unknown) {
  console.log(`${label} err instanceof Error:`, err instanceof Error);
  console.log(`${label} err instanceof RangeError:`, err instanceof RangeError);
  console.log(`${label} err name:`, valueText(err?.name));
  console.log(`${label} keys:`, Object.keys(err ?? {}).join(","));
  console.log(`${label} code:`, valueText(err?.code));
  console.log(`${label} killed:`, valueText(err?.killed));
  console.log(`${label} signal:`, valueText(err?.signal));
  console.log(`${label} cmd:`, valueText(err?.cmd));
  console.log(`${label} stdout:`, JSON.stringify(String(stdout)));
  console.log(`${label} stderr:`, JSON.stringify(String(stderr)));
}

exec("printf 123456", { maxBuffer: 3, encoding: "utf8" }, (err, stdout, stderr) => {
  report("exec max", err, stdout, stderr);
  execFile(
    "sh",
    ["-c", "printf abcdef >&2"],
    { maxBuffer: 3, encoding: "utf8" },
    (err, stdout, stderr) => {
      report("execFile max", err, stdout, stderr);
      exec("sleep 1", { timeout: 50, encoding: "utf8" }, (err, stdout, stderr) => {
        report("exec timeout", err, stdout, stderr);
        execFile(
          "sh",
          ["-c", "sleep 1"],
          { timeout: 50, encoding: "utf8" },
          (err, stdout, stderr) => {
            report("execFile timeout", err, stdout, stderr);
          },
        );
      });
    },
  );
});
