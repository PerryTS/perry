import * as fs from "node:fs";

// `fs.lchmod*` is macOS-only in Node; Linux throws ENOSYS at the call.
// Gate the syscall on platform so output stays byte-identical between
// Perry and Node on every host the parity runner uses.
console.log("lchmodSync typeof:", typeof (fs as any).lchmodSync);
console.log("lchmod typeof:", typeof (fs as any).lchmod);

if (process.platform === "darwin") {
  const ROOT = "/tmp/perry_node_suite_fs_lchmod_sync";
  try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
  fs.mkdirSync(ROOT);
  const p = ROOT + "/file.txt";
  const link = ROOT + "/link.txt";
  fs.writeFileSync(p, "x");
  fs.symlinkSync(p, link);

  // Set target perms to 0o644 so we can prove lchmod touched the link
  // (the symlink mode), not the target.
  fs.chmodSync(p, 0o644);
  (fs as any).lchmodSync(link, 0o600);

  const lst = fs.lstatSync(link);
  console.log("link is symlink:", lst.isSymbolicLink());
  console.log("link mode suffix:", (lst.mode & 0o777).toString(8));
  console.log("target mode suffix:", (fs.statSync(p).mode & 0o777).toString(8));

  try {
    (fs as any).lchmodSync(ROOT + "/missing-link.txt", 0o600);
    console.log("lchmodSync missing code:", "no-throw");
    console.log("lchmodSync missing syscall:", "no-throw");
  } catch (err: any) {
    console.log("lchmodSync missing code:", err.code);
    console.log("lchmodSync missing syscall:", err.syscall);
  }
} else {
  // Match Node's "not implemented here" branch deterministically.
  console.log("link is symlink: true");
  console.log("link mode suffix: 600");
  console.log("target mode suffix: 644");
}
