import { exec, execFile, execFileSync, execSync, spawn, spawnSync } from "node:child_process";

function report(label, run) {
  try {
    run();
    console.log(`${label}: ok`);
  } catch (err) {
    console.log(`${label}:`, err.constructor.name, err.code, err.message);
  }
}

report("exec command undefined", () => exec(undefined));
report("exec command number", () => exec(123));
report("execSync command undefined", () => execSync(undefined));
report("execSync command number", () => execSync(123));
report("execFile file undefined", () => execFile(undefined));
report("execFile file number", () => execFile(123));
report("execFile args string", () => execFile("echo", "-n"));
report("execFileSync file undefined", () => execFileSync(undefined));
report("execFileSync file number", () => execFileSync(123));
report("execFileSync args string", () => execFileSync("echo", "-n"));
report("spawn file undefined", () => spawn(undefined));
report("spawn file number", () => spawn(123));
report("spawn args string", () => spawn("echo", "-n"));
report("spawnSync file undefined", () => spawnSync(undefined));
report("spawnSync file number", () => spawnSync(123));
report("spawnSync args string", () => spawnSync("echo", "-n"));
