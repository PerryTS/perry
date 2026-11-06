import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_path_mutator_errors";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const file = ROOT + "/file.txt";
const src = ROOT + "/src.txt";
const dest = ROOT + "/dest.txt";
const missing = ROOT + "/missing.txt";
const noParent = ROOT + "/no-parent/out.txt";

fs.writeFileSync(file, "file");
fs.writeFileSync(src, "src");
fs.writeFileSync(dest, "dest");
fs.symlinkSync("file.txt", ROOT + "/link.txt");

function showUnary(label: string, err: unknown, code: string, syscall: string, path: string) {
  const fsErr = err as any;
  console.log(label + " is Error:", err instanceof Error);
  console.log(label + " code:", fsErr && fsErr.code);
  console.log(label + " code matches:", fsErr && fsErr.code === code);
  console.log(label + " syscall:", fsErr && fsErr.syscall);
  console.log(label + " syscall matches:", fsErr && fsErr.syscall === syscall);
  console.log(label + " path matches:", fsErr && fsErr.path === path);
}

function showPair(
  label: string,
  err: unknown,
  code: string,
  syscall: string,
  path: string,
  destPath: string,
) {
  showUnary(label, err, code, syscall, path);
  const fsErr = err as any;
  console.log(label + " dest matches:", fsErr && fsErr.dest === destPath);
}

try {
  fs.readlinkSync(missing);
  console.log("readlink sync missing unexpectedly succeeded");
} catch (err) {
  showUnary("readlink sync missing", err, "ENOENT", "readlink", missing);
}

try {
  fs.readlinkSync(file);
  console.log("readlink sync regular unexpectedly succeeded");
} catch (err) {
  showUnary("readlink sync regular", err, "EINVAL", "readlink", file);
}

try {
  fs.renameSync(missing, ROOT + "/renamed-sync.txt");
  console.log("rename sync missing src unexpectedly succeeded");
} catch (err) {
  showPair("rename sync missing src", err, "ENOENT", "rename", missing, ROOT + "/renamed-sync.txt");
}

try {
  fs.renameSync(src, noParent);
  console.log("rename sync missing dest parent unexpectedly succeeded");
} catch (err) {
  showPair("rename sync missing dest parent", err, "ENOENT", "rename", src, noParent);
}

try {
  fs.copyFileSync(missing, ROOT + "/copy-sync.txt");
  console.log("copyFile sync missing src unexpectedly succeeded");
} catch (err) {
  showPair("copyFile sync missing src", err, "ENOENT", "copyfile", missing, ROOT + "/copy-sync.txt");
}

try {
  fs.copyFileSync(src, noParent);
  console.log("copyFile sync missing dest parent unexpectedly succeeded");
} catch (err) {
  showPair("copyFile sync missing dest parent", err, "ENOENT", "copyfile", src, noParent);
}

try {
  fs.copyFileSync(src, dest, fs.constants.COPYFILE_EXCL);
  console.log("copyFile sync excl unexpectedly succeeded");
} catch (err) {
  showPair("copyFile sync excl", err, "EEXIST", "copyfile", src, dest);
}

try {
  fs.linkSync(missing, ROOT + "/hard-sync.txt");
  console.log("link sync missing src unexpectedly succeeded");
} catch (err) {
  showPair("link sync missing src", err, "ENOENT", "link", missing, ROOT + "/hard-sync.txt");
}

try {
  fs.linkSync(src, noParent);
  console.log("link sync missing dest parent unexpectedly succeeded");
} catch (err) {
  showPair("link sync missing dest parent", err, "ENOENT", "link", src, noParent);
}

try {
  fs.symlinkSync("target.txt", noParent);
  console.log("symlink sync missing dest parent unexpectedly succeeded");
} catch (err) {
  showPair("symlink sync missing dest parent", err, "ENOENT", "symlink", "target.txt", noParent);
}

try {
  fs.symlinkSync("target.txt", dest);
  console.log("symlink sync existing dest unexpectedly succeeded");
} catch (err) {
  showPair("symlink sync existing dest", err, "EEXIST", "symlink", "target.txt", dest);
}

await new Promise<void>((resolve) => {
  fs.readlink(missing, (err) => {
    showUnary("readlink callback missing", err, "ENOENT", "readlink", missing);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.readlink(file, (err) => {
    showUnary("readlink callback regular", err, "EINVAL", "readlink", file);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.rename(missing, ROOT + "/renamed-callback.txt", (err) => {
    showPair("rename callback missing src", err, "ENOENT", "rename", missing, ROOT + "/renamed-callback.txt");
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.rename(src, noParent, (err) => {
    showPair("rename callback missing dest parent", err, "ENOENT", "rename", src, noParent);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.copyFile(missing, ROOT + "/copy-callback.txt", (err) => {
    showPair("copyFile callback missing src", err, "ENOENT", "copyfile", missing, ROOT + "/copy-callback.txt");
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.copyFile(src, noParent, (err) => {
    showPair("copyFile callback missing dest parent", err, "ENOENT", "copyfile", src, noParent);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.copyFile(src, dest, fs.constants.COPYFILE_EXCL, (err) => {
    showPair("copyFile callback excl", err, "EEXIST", "copyfile", src, dest);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.link(missing, ROOT + "/hard-callback.txt", (err) => {
    showPair("link callback missing src", err, "ENOENT", "link", missing, ROOT + "/hard-callback.txt");
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.link(src, noParent, (err) => {
    showPair("link callback missing dest parent", err, "ENOENT", "link", src, noParent);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.symlink("target.txt", noParent, (err) => {
    showPair("symlink callback missing dest parent", err, "ENOENT", "symlink", "target.txt", noParent);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.symlink("target.txt", dest, (err) => {
    showPair("symlink callback existing dest", err, "EEXIST", "symlink", "target.txt", dest);
    resolve();
  });
});

try {
  await fsp.readlink(missing);
  console.log("readlink promise missing unexpectedly resolved");
} catch (err) {
  showUnary("readlink promise missing", err, "ENOENT", "readlink", missing);
}

try {
  await fsp.readlink(file);
  console.log("readlink promise regular unexpectedly resolved");
} catch (err) {
  showUnary("readlink promise regular", err, "EINVAL", "readlink", file);
}

try {
  await fsp.rename(missing, ROOT + "/renamed-promise.txt");
  console.log("rename promise missing src unexpectedly resolved");
} catch (err) {
  showPair("rename promise missing src", err, "ENOENT", "rename", missing, ROOT + "/renamed-promise.txt");
}

try {
  await fsp.rename(src, noParent);
  console.log("rename promise missing dest parent unexpectedly resolved");
} catch (err) {
  showPair("rename promise missing dest parent", err, "ENOENT", "rename", src, noParent);
}

try {
  await fsp.copyFile(missing, ROOT + "/copy-promise.txt");
  console.log("copyFile promise missing src unexpectedly resolved");
} catch (err) {
  showPair("copyFile promise missing src", err, "ENOENT", "copyfile", missing, ROOT + "/copy-promise.txt");
}

try {
  await fsp.copyFile(src, noParent);
  console.log("copyFile promise missing dest parent unexpectedly resolved");
} catch (err) {
  showPair("copyFile promise missing dest parent", err, "ENOENT", "copyfile", src, noParent);
}

try {
  await fsp.copyFile(src, dest, fs.constants.COPYFILE_EXCL);
  console.log("copyFile promise excl unexpectedly resolved");
} catch (err) {
  showPair("copyFile promise excl", err, "EEXIST", "copyfile", src, dest);
}

try {
  await fsp.link(missing, ROOT + "/hard-promise.txt");
  console.log("link promise missing src unexpectedly resolved");
} catch (err) {
  showPair("link promise missing src", err, "ENOENT", "link", missing, ROOT + "/hard-promise.txt");
}

try {
  await fsp.link(src, noParent);
  console.log("link promise missing dest parent unexpectedly resolved");
} catch (err) {
  showPair("link promise missing dest parent", err, "ENOENT", "link", src, noParent);
}

try {
  await fsp.symlink("target.txt", noParent);
  console.log("symlink promise missing dest parent unexpectedly resolved");
} catch (err) {
  showPair("symlink promise missing dest parent", err, "ENOENT", "symlink", "target.txt", noParent);
}

try {
  await fsp.symlink("target.txt", dest);
  console.log("symlink promise existing dest unexpectedly resolved");
} catch (err) {
  showPair("symlink promise existing dest", err, "EEXIST", "symlink", "target.txt", dest);
}

console.log("readlink sync success:", fs.readlinkSync(ROOT + "/link.txt"));

const renameOkSrc = ROOT + "/rename-ok-src.txt";
const renameOkDst = ROOT + "/rename-ok-dst.txt";
fs.writeFileSync(renameOkSrc, "rename");
fs.renameSync(renameOkSrc, renameOkDst);
console.log("rename sync success:", fs.readFileSync(renameOkDst, "utf8"));

const copyOkDst = ROOT + "/copy-ok.txt";
fs.copyFileSync(file, copyOkDst);
console.log("copyFile sync success:", fs.readFileSync(copyOkDst, "utf8"));

const hardOk = ROOT + "/hard-ok.txt";
fs.linkSync(file, hardOk);
console.log("link sync success:", fs.readFileSync(hardOk, "utf8"));

const symOk = ROOT + "/sym-ok.txt";
fs.symlinkSync("file.txt", symOk);
console.log("symlink sync success:", fs.readlinkSync(symOk));

try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
