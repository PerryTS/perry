import { exec, execFile } from "node:child_process";

function report(label: string, err: unknown, stdout: any, stderr: any) {
  console.log(`${label} err:`, err === null ? "null" : (err as any)?.constructor?.name);
  console.log(`${label} stdout isBuffer:`, Buffer.isBuffer(stdout));
  console.log(`${label} stdout ctor:`, stdout?.constructor?.name);
  console.log(`${label} stdout text:`, stdout.toString("utf8"));
  console.log(`${label} stdout hex:`, stdout.toString("hex"));
  console.log(`${label} stderr isBuffer:`, Buffer.isBuffer(stderr));
  console.log(`${label} stderr text:`, stderr.toString("utf8"));
  console.log(`${label} stderr hex:`, stderr.toString("hex"));
}

exec("printf out; printf err >&2", { encoding: "buffer" }, (err, stdout, stderr) => {
  report("exec-buffer", err, stdout, stderr);
  execFile(
    "sh",
    ["-c", "printf file; printf ferr >&2"],
    { encoding: null },
    (err, stdout, stderr) => {
      report("execFile-null", err, stdout, stderr);
      exec("printf str; printf serr >&2", (err, stdout, stderr) => {
        console.log("exec-default err:", err === null ? "null" : (err as any)?.constructor?.name);
        console.log("exec-default stdout isBuffer:", Buffer.isBuffer(stdout));
        console.log("exec-default stdout:", stdout);
        console.log("exec-default stderr:", stderr);
      });
    },
  );
});
