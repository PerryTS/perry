import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_callback_mkdir_options";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}

await new Promise<void>((resolve) => {
  fs.mkdir(ROOT + "/a/b", { recursive: true }, (err) => {
    console.log("mkdir callback recursive err:", err === null);
    console.log("mkdir callback recursive dir:", fs.statSync(ROOT + "/a/b").isDirectory());
    resolve();
  });
});


await new Promise<void>((resolve) => {
  fs.mkdir(ROOT + "/mode", 448, (err) => {
    console.log("mkdir callback mode err:", err === null);
    console.log("mkdir callback mode:", (fs.statSync(ROOT + "/mode").mode & 0o777).toString(8));
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.mkdir(new URL("file://" + ROOT + "/url%20dir"), { recursive: true }, (err) => {
    console.log("mkdir callback url err:", err === null);
    console.log("mkdir callback url dir:", fs.statSync(ROOT + "/url dir").isDirectory());
    resolve();
  });
});
