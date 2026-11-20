import { exec, execFile } from "node:child_process";

function report(label: string, err: any, stdout: unknown, stderr: unknown) {
  console.log(`${label} err instanceof Error:`, err instanceof Error);
  console.log(`${label} keys:`, Object.keys(err).join(","));
  console.log(`${label} code:`, err.code);
  console.log(`${label} killed:`, err.killed);
  console.log(`${label} signal:`, err.signal);
  console.log(`${label} cmd:`, err.cmd);
  console.log(`${label} stdout:`, String(stdout));
  console.log(`${label} stderr:`, String(stderr));
}

exec("printf out; printf err >&2; exit 3", (err, stdout, stderr) => {
  report("exec", err, stdout, stderr);
  execFile("sh", ["-c", "printf out; printf err >&2; exit 4"], (err, stdout, stderr) => {
    report("execFile", err, stdout, stderr);
  });
});
