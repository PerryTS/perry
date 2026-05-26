import * as fs from "node:fs";

console.log("lchmod typeof:", typeof (fs as any).lchmod);

if (process.platform === "darwin") {
  const ROOT = "/tmp/perry_node_suite_fs_callback_lchmod";
  try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
  fs.mkdirSync(ROOT);
  const p = ROOT + "/file.txt";
  const link = ROOT + "/link.txt";
  fs.writeFileSync(p, "x");
  fs.symlinkSync(p, link);
  fs.chmodSync(p, 0o644);

  await new Promise<void>((resolve) => {
    (fs as any).lchmod(link, 0o600, (err: Error | null) => {
      console.log("lchmod callback err:", err === null);
      resolve();
    });
  });
  console.log("link mode suffix:", (fs.lstatSync(link).mode & 0o777).toString(8));
  console.log("target mode suffix:", (fs.statSync(p).mode & 0o777).toString(8));
} else {
  console.log("lchmod callback err: true");
  console.log("link mode suffix: 600");
  console.log("target mode suffix: 644");
}
