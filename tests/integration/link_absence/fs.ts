// HWS hook: js_nm_install_fs
// HWS prefixes: perry_runtime::fs::|js_fs_
import fs from "node:fs";
console.log(Object.keys(fs).includes("readFileSync"));
console.log(fs.readFileSync("value.json", "utf8").trim());
