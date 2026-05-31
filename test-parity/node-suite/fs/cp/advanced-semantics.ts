import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_cp_advanced_semantics";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

function reset(name: string) {
  const dir = ROOT + "/" + name;
  try { fs.rmSync(dir, { recursive: true, force: true }); } catch (_e) {}
  fs.mkdirSync(dir, { recursive: true });
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
  const dir = reset("sync-subdir");
  fs.mkdirSync(dir + "/src");
  fs.writeFileSync(dir + "/src/file.txt", "file");
  try {
    fs.cpSync(dir + "/src", dir + "/src/child", { recursive: true });
    console.log("cp sync subdir unexpectedly succeeded");
  } catch (err) {
    showCode("cp sync subdir", err, "ERR_FS_CP_EINVAL");
  }
  console.log("cp sync subdir not created:", !fs.existsSync(dir + "/src/child"));
}

{
  const dir = reset("sync-conflicts");
  fs.writeFileSync(dir + "/file.txt", "file");
  fs.mkdirSync(dir + "/src-dir");
  fs.mkdirSync(dir + "/dest-dir");
  fs.writeFileSync(dir + "/dest-file.txt", "dest");
  try {
    fs.cpSync(dir + "/file.txt", dir + "/file.txt");
    console.log("cp sync same file unexpectedly succeeded");
  } catch (err) {
    showCode("cp sync same file", err, "ERR_FS_CP_EINVAL");
  }
  try {
    fs.cpSync(dir + "/src-dir", dir + "/out-dir");
    console.log("cp sync dir no recursive unexpectedly succeeded");
  } catch (err) {
    showCode("cp sync dir no recursive", err, "ERR_FS_EISDIR");
  }
  try {
    fs.cpSync(dir + "/src-dir", dir + "/dest-file.txt", { recursive: true });
    console.log("cp sync dir to file unexpectedly succeeded");
  } catch (err) {
    showCode("cp sync dir to file", err, "ERR_FS_CP_DIR_TO_NON_DIR");
  }
  try {
    fs.cpSync(dir + "/file.txt", dir + "/dest-dir");
    console.log("cp sync file to dir unexpectedly succeeded");
  } catch (err) {
    showCode("cp sync file to dir", err, "ERR_FS_CP_NON_DIR_TO_DIR");
  }
}

{
  const dir = reset("sync-exists");
  const src = dir + "/src.txt";
  const dest = dir + "/dest.txt";
  fs.writeFileSync(src, "new");
  fs.writeFileSync(dest, "old");
  try {
    fs.cpSync(src, dest, { force: false, errorOnExist: true });
    console.log("cp sync errorOnExist unexpectedly succeeded");
  } catch (err) {
    showCpExist("cp sync errorOnExist", err, dest);
  }
  console.log("cp sync errorOnExist preserved:", fs.readFileSync(dest, "utf8"));
  fs.cpSync(src, dest, { force: false });
  console.log("cp sync force false skipped:", fs.readFileSync(dest, "utf8"));
}

{
  const dir = reset("sync-filter-mode");
  fs.mkdirSync(dir + "/src");
  fs.writeFileSync(dir + "/src/file.txt", "file");
  try {
    fs.cpSync(dir + "/src", dir + "/dst", {
      recursive: true,
      filter: () => Promise.resolve(true),
    });
    console.log("cp sync promise filter unexpectedly succeeded");
  } catch (err) {
    showCode("cp sync promise filter", err, "ERR_INVALID_RETURN_VALUE");
  }
  fs.cpSync(dir + "/src/file.txt", dir + "/mode-copy.txt", {
    mode: fs.constants.COPYFILE_FICLONE,
  });
  console.log("cp sync mode ficlone copied:", fs.readFileSync(dir + "/mode-copy.txt", "utf8"));
}

{
  const dir = reset("sync-eloop");
  fs.symlinkSync("self-link", dir + "/self-link");
  try {
    fs.cpSync(dir + "/self-link", dir + "/out", { recursive: true, dereference: true });
    console.log("cp sync eloop unexpectedly succeeded");
  } catch (err) {
    showCode("cp sync eloop", err, "ELOOP");
    const fsErr = err as any;
    console.log("cp sync eloop syscall:", fsErr && fsErr.syscall);
  }
}

{
  const dir = reset("callback-filter");
  fs.mkdirSync(dir + "/src/a", { recursive: true });
  fs.writeFileSync(dir + "/src/keep.txt", "keep");
  fs.writeFileSync(dir + "/src/drop.md", "drop");
  fs.writeFileSync(dir + "/src/a/nested.txt", "nested");
  fs.writeFileSync(dir + "/src/a/nested.md", "nested drop");
  let calls = 0;
  await new Promise<void>((resolve) => {
    fs.cp(dir + "/src", dir + "/dst", {
      recursive: true,
      filter: (src) => {
        calls++;
        return Promise.resolve(fs.statSync(src).isDirectory() || src.endsWith(".txt"));
      },
    }, (err) => {
      console.log("cp callback async filter err null:", err === null);
      console.log("cp callback async filter called:", calls > 0);
      console.log("cp callback async filter keep:", fs.existsSync(dir + "/dst/keep.txt"));
      console.log("cp callback async filter drop:", fs.existsSync(dir + "/dst/drop.md"));
      console.log("cp callback async filter nested keep:", fs.existsSync(dir + "/dst/a/nested.txt"));
      console.log("cp callback async filter nested drop:", fs.existsSync(dir + "/dst/a/nested.md"));
      resolve();
    });
  });
}

{
  const dir = reset("callback-error");
  const src = dir + "/src.txt";
  const dest = dir + "/dest.txt";
  fs.writeFileSync(src, "new");
  fs.writeFileSync(dest, "old");
  await new Promise<void>((resolve) => {
    fs.cp(src, dest, { force: false, errorOnExist: true }, (err) => {
      showCpExist("cp callback errorOnExist", err, dest);
      console.log("cp callback errorOnExist preserved:", fs.readFileSync(dest, "utf8"));
      resolve();
    });
  });
}

try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
