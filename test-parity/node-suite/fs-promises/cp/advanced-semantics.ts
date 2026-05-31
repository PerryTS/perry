import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_cp_advanced_semantics";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

async function reset(name: string) {
  const dir = ROOT + "/" + name;
  try { await fsp.rm(dir, { recursive: true, force: true }); } catch (_e) {}
  await fsp.mkdir(dir, { recursive: true });
  return dir;
}

function showCode(label: string, err: unknown, code: string) {
  const fsErr = err as any;
  console.log(label + " is Error:", err instanceof Error);
  console.log(label + " code:", fsErr && fsErr.code);
  console.log(label + " code matches:", fsErr && fsErr.code === code);
}

function showCpExist(label: string, err: unknown, path: string) {
  showCode(label, err, "ERR_FS_CP_EEXIST");
  const fsErr = err as any;
  console.log(label + " syscall matches:", fsErr && fsErr.syscall === "cp");
  console.log(label + " path matches:", fsErr && fsErr.path === path);
  console.log(label + " errno number:", typeof (fsErr && fsErr.errno) === "number");
}

{
  const dir = await reset("promise-subdir");
  await fsp.mkdir(dir + "/src");
  await fsp.writeFile(dir + "/src/file.txt", "file");
  try {
    await fsp.cp(dir + "/src", dir + "/src/child", { recursive: true });
    console.log("cp promise subdir unexpectedly resolved");
  } catch (err) {
    showCode("cp promise subdir", err, "ERR_FS_CP_EINVAL");
  }
  console.log("cp promise subdir not created:", !fs.existsSync(dir + "/src/child"));
}

{
  const dir = await reset("promise-conflicts");
  await fsp.writeFile(dir + "/file.txt", "file");
  await fsp.mkdir(dir + "/src-dir");
  await fsp.mkdir(dir + "/dest-dir");
  await fsp.writeFile(dir + "/dest-file.txt", "dest");
  try {
    await fsp.cp(dir + "/file.txt", dir + "/file.txt");
    console.log("cp promise same file unexpectedly resolved");
  } catch (err) {
    showCode("cp promise same file", err, "ERR_FS_CP_EINVAL");
  }
  try {
    await fsp.cp(dir + "/src-dir", dir + "/out-dir");
    console.log("cp promise dir no recursive unexpectedly resolved");
  } catch (err) {
    showCode("cp promise dir no recursive", err, "ERR_FS_EISDIR");
  }
  try {
    await fsp.cp(dir + "/src-dir", dir + "/dest-file.txt", { recursive: true });
    console.log("cp promise dir to file unexpectedly resolved");
  } catch (err) {
    showCode("cp promise dir to file", err, "ERR_FS_CP_DIR_TO_NON_DIR");
  }
  try {
    await fsp.cp(dir + "/file.txt", dir + "/dest-dir");
    console.log("cp promise file to dir unexpectedly resolved");
  } catch (err) {
    showCode("cp promise file to dir", err, "ERR_FS_CP_NON_DIR_TO_DIR");
  }
}

{
  const dir = await reset("promise-exists");
  const src = dir + "/src.txt";
  const dest = dir + "/dest.txt";
  await fsp.writeFile(src, "new");
  await fsp.writeFile(dest, "old");
  try {
    await fsp.cp(src, dest, { force: false, errorOnExist: true });
    console.log("cp promise errorOnExist unexpectedly resolved");
  } catch (err) {
    showCpExist("cp promise errorOnExist", err, dest);
  }
  console.log("cp promise errorOnExist preserved:", await fsp.readFile(dest, "utf8"));
  await fsp.cp(src, dest, { force: false });
  console.log("cp promise force false skipped:", await fsp.readFile(dest, "utf8"));
}

{
  const dir = await reset("promise-filter-mode");
  await fsp.mkdir(dir + "/src/a", { recursive: true });
  await fsp.writeFile(dir + "/src/keep.txt", "keep");
  await fsp.writeFile(dir + "/src/drop.md", "drop");
  await fsp.writeFile(dir + "/src/a/nested.txt", "nested");
  await fsp.writeFile(dir + "/src/a/nested.md", "nested drop");
  let calls = 0;
  await fsp.cp(dir + "/src", dir + "/dst", {
    recursive: true,
    filter: (src) => {
      calls++;
      return Promise.resolve(fs.statSync(src).isDirectory() || src.endsWith(".txt"));
    },
  });
  console.log("cp promise async filter called:", calls > 0);
  console.log("cp promise async filter keep:", fs.existsSync(dir + "/dst/keep.txt"));
  console.log("cp promise async filter drop:", fs.existsSync(dir + "/dst/drop.md"));
  console.log("cp promise async filter nested keep:", fs.existsSync(dir + "/dst/a/nested.txt"));
  console.log("cp promise async filter nested drop:", fs.existsSync(dir + "/dst/a/nested.md"));

  await fsp.cp(dir + "/src/keep.txt", dir + "/mode-copy.txt", {
    mode: fs.constants.COPYFILE_FICLONE,
  });
  console.log("cp promise mode ficlone copied:", await fsp.readFile(dir + "/mode-copy.txt", "utf8"));
}

{
  const dir = await reset("promise-eloop");
  await fsp.symlink("self-link", dir + "/self-link");
  try {
    await fsp.cp(dir + "/self-link", dir + "/out", { recursive: true, dereference: true });
    console.log("cp promise eloop unexpectedly resolved");
  } catch (err) {
    showCode("cp promise eloop", err, "ELOOP");
    const fsErr = err as any;
    console.log("cp promise eloop syscall:", fsErr && fsErr.syscall);
  }
}

try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
