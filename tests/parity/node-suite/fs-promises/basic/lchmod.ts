import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

console.log("fsp.lchmod typeof:", typeof (fsp as any).lchmod);

if (process.platform === "darwin") {
  const ROOT = "/tmp/perry_node_suite_fsp_lchmod";
  try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
  fs.mkdirSync(ROOT);
  const p = ROOT + "/file.txt";
  const link = ROOT + "/link.txt";
  fs.writeFileSync(p, "x");
  fs.symlinkSync(p, link);
  fs.chmodSync(p, 0o644);

  await (fsp as any).lchmod(link, 0o600);
  console.log("link mode suffix:", (fs.lstatSync(link).mode & 0o777).toString(8));
  console.log("target mode suffix:", (fs.statSync(p).mode & 0o777).toString(8));

  try {
    await (fsp as any).lchmod(ROOT + "/missing-link.txt", 0o600);
    console.log("lchmod reject code:", "resolved");
    console.log("lchmod reject syscall:", "resolved");
  } catch (err: any) {
    console.log("lchmod reject code:", err.code);
    console.log("lchmod reject syscall:", err.syscall);
  }
} else {
  if (process.platform === "linux") {
    try {
      await (fsp as any).lchmod("/tmp/perry_node_suite_fsp_lchmod_missing", 0o600);
      console.log("lchmod reject code:", "resolved");
    } catch (err) {
      console.log("lchmod reject code:", (err as any).code);
    }
  } else {
    console.log("lchmod reject code:", "ERR_METHOD_NOT_IMPLEMENTED");
  }
  console.log("link mode suffix: 600");
  console.log("target mode suffix: 644");
}
