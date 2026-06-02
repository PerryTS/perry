import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_callback_cp_rm_options";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const src = ROOT + "/src.txt";
const dst = ROOT + "/dst.txt";
fs.writeFileSync(src, "replacement");
fs.writeFileSync(dst, "original");

fs.cp(src, dst, { force: false }, (err) => {
  console.log("cp force false callback err:", err === null);
  console.log("cp force false callback content:", fs.readFileSync(dst, "utf8"));
  fs.rm(ROOT + "/missing", { force: true }, (rmErr) => {
    console.log("rm missing force callback err:", rmErr === null);
  });
});
