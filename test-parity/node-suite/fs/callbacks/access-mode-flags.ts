import * as fs from "node:fs";

// access() with each of F_OK/R_OK/W_OK/X_OK on a regular file. The
// chmod ensures predictable bits across machines.
const ROOT = "/tmp/perry_node_suite_fs_access_mode_flags";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const path = ROOT + "/file.txt";
fs.writeFileSync(path, "abc");
fs.chmodSync(path, 0o644);

const check = (label: string, mode: number) => {
  let ok = false;
  try { fs.accessSync(path, mode); ok = true; } catch (_e) { ok = false; }
  console.log(`access ${label}:`, ok);
};

check("F_OK", fs.constants.F_OK);
check("R_OK", fs.constants.R_OK);
check("W_OK", fs.constants.W_OK);
// X_OK on a 0o644 file should fail.
let xOk = true;
try { fs.accessSync(path, fs.constants.X_OK); } catch (_e) { xOk = false; }
console.log("access X_OK on 0o644:", xOk);

// X_OK on a 0o755 file should succeed.
fs.chmodSync(path, 0o755);
let xOk2 = false;
try { fs.accessSync(path, fs.constants.X_OK); xOk2 = true; } catch (_e) { xOk2 = false; }
console.log("access X_OK on 0o755:", xOk2);
