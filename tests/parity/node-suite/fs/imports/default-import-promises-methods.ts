// `fs.promises.<method>()` reached as a member of the default `node:fs`
// import: every method runs and settles like Node's, not only the handful
// with dedicated lowering (readFile/writeFile/appendFile/mkdir/rmdir).
import fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_default_import_promises_methods";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
fs.writeFileSync(ROOT + "/a.txt", "abc");

async function main() {
  console.log("readdir:", JSON.stringify(await fs.promises.readdir(ROOT)));
  console.log("access:", await fs.promises.access(ROOT + "/a.txt"));
  const st = await fs.promises.stat(ROOT + "/a.txt");
  console.log("stat size:", st.size, st.isFile());
  const lst = await fs.promises.lstat(ROOT + "/a.txt");
  console.log("lstat isFile:", lst.isFile());
  await fs.promises.copyFile(ROOT + "/a.txt", ROOT + "/copy.txt");
  console.log("copyFile:", fs.readFileSync(ROOT + "/copy.txt", "utf8"));
  await fs.promises.rename(ROOT + "/copy.txt", ROOT + "/moved.txt");
  console.log("rename:", fs.existsSync(ROOT + "/copy.txt"), fs.existsSync(ROOT + "/moved.txt"));
  await fs.promises.truncate(ROOT + "/moved.txt", 1);
  console.log("truncate:", fs.readFileSync(ROOT + "/moved.txt", "utf8"));
  await fs.promises.unlink(ROOT + "/moved.txt");
  console.log("unlink:", fs.existsSync(ROOT + "/moved.txt"));
  console.log("realpath is string:", typeof (await fs.promises.realpath(ROOT)) === "string");
  const dir = await fs.promises.mkdtemp(ROOT + "/tmp-");
  console.log("mkdtemp prefix:", dir.startsWith(ROOT + "/tmp-"));
  await fs.promises.rm(dir, { recursive: true });
  console.log("rm:", fs.existsSync(dir));
  try {
    await fs.promises.stat(ROOT + "/missing");
    console.log("stat missing: resolved");
  } catch (e: any) {
    console.log("stat missing:", e.code, e.syscall);
  }
  fs.rmSync(ROOT, { recursive: true, force: true });
}
main();
