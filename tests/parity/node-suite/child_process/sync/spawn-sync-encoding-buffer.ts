import { spawnSync } from "node:child_process";

function report(label: string, result: any) {
  console.log(`${label} stdout isBuffer:`, Buffer.isBuffer(result.stdout));
  console.log(`${label} stdout ctor:`, result.stdout?.constructor?.name);
  console.log(`${label} stdout text:`, result.stdout?.toString("utf8"));
  console.log(`${label} stdout hex:`, result.stdout?.toString("hex"));
  console.log(`${label} stderr isBuffer:`, Buffer.isBuffer(result.stderr));
  console.log(`${label} stderr ctor:`, result.stderr?.constructor?.name);
  console.log(`${label} stderr text:`, result.stderr?.toString("utf8"));
  console.log(`${label} stderr hex:`, result.stderr?.toString("hex"));
}

report("spawnSync-default", spawnSync("sh", ["-c", "printf out; printf err >&2"]));
report(
  "spawnSync-buffer",
  spawnSync("sh", ["-c", "printf bout; printf berr >&2"], { encoding: "buffer" }),
);
report(
  "spawnSync-null",
  spawnSync("sh", ["-c", "printf nout; printf nerr >&2"], { encoding: null }),
);

const text = spawnSync("sh", ["-c", "printf text; printf terr >&2"], { encoding: "utf8" });
console.log("spawnSync-utf8 stdout isBuffer:", Buffer.isBuffer(text.stdout));
console.log("spawnSync-utf8 stdout:", text.stdout);
console.log("spawnSync-utf8 stderr isBuffer:", Buffer.isBuffer(text.stderr));
console.log("spawnSync-utf8 stderr:", text.stderr);
