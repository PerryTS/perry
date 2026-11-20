import {
  exec,
  execFile,
  execFileSync,
  execSync,
  fork,
  spawn,
  spawnSync,
} from "node:child_process";

function report(label: string, action: () => unknown) {
  try {
    const child: any = action();
    if (child && typeof child === "object") {
      child.on?.("error", () => {});
      if (child.connected) child.disconnect();
      child.kill?.();
    }
    console.log(`${label}: no throw`);
  } catch (error: any) {
    console.log(`${label}:`, error?.constructor?.name, error?.code);
  }
}

report("exec command", () => exec("node\0 --version"));
report("execSync command", () => execSync("node\0 --version"));
report("execFile file", () => execFile("node\0", []));
report("execFile arg", () => execFile("node", ["bad\0arg"]));
report("execFileSync file", () => execFileSync("node\0", []));
report("execFileSync arg", () => execFileSync("node", ["bad\0arg"]));
report("spawn file", () => spawn("node\0", []));
report("spawn arg", () => spawn("node", ["bad\0arg"]));
report("spawnSync file", () => spawnSync("node\0", []));
report("spawnSync arg", () => spawnSync("node", ["bad\0arg"]));
report("fork module", () => fork("bad\0module.js"));
