import { execFileSync, execSync } from "node:child_process";

function report(label: string, value: any) {
  console.log(`${label} isBuffer:`, Buffer.isBuffer(value));
  console.log(`${label} ctor:`, value?.constructor?.name);
  console.log(`${label} text:`, value.toString("utf8"));
  console.log(`${label} hex:`, value.toString("hex"));
}

report("execSync-default", execSync("printf def"));
report("execSync-buffer", execSync("printf out", { encoding: "buffer" }));
report("execSync-null", execSync("printf nul", { encoding: null }));
report("execFileSync-default", execFileSync("sh", ["-c", "printf file"]));
report(
  "execFileSync-buffer",
  execFileSync("sh", ["-c", "printf buff"], { encoding: "buffer" }),
);
report(
  "execFileSync-null",
  execFileSync("sh", ["-c", "printf nil"], { encoding: null }),
);

const execText = execSync("printf text", { encoding: "utf8" });
console.log("execSync-utf8 isBuffer:", Buffer.isBuffer(execText));
console.log("execSync-utf8:", execText);

const execFileText = execFileSync("sh", ["-c", "printf text-file"], { encoding: "utf8" });
console.log("execFileSync-utf8 isBuffer:", Buffer.isBuffer(execFileText));
console.log("execFileSync-utf8:", execFileText);
