import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_callback_file_url_paths";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const file = ROOT + "/file url.txt";
const copy = ROOT + "/copy url.txt";
const fileUrl = new URL("file://" + file.replace(" ", "%20"));
const copyUrl = new URL("file://" + copy.replace(" ", "%20"));
fs.writeFileSync(file, "url-data");

await new Promise<void>((resolve) => {
  fs.readFile(fileUrl, "utf8", (err, data) => {
    console.log("url readFile callback err:", err === null);
    console.log("url readFile callback data:", data);
    resolve();
  });
});
await new Promise<void>((resolve) => {
  fs.copyFile(fileUrl, copyUrl, (err) => {
    console.log("url copyFile callback err:", err === null);
    resolve();
  });
});
console.log("url copyFile callback content:", fs.readFileSync(copy, "utf8"));
await new Promise<void>((resolve) => {
  fs.stat(fileUrl, (err, st) => {
    console.log("url stat callback err:", err === null);
    console.log("url stat callback isFile:", st.isFile());
    resolve();
  });
});
