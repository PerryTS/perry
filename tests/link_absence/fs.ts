// HWS hook: js_nm_install_fs
// HWS prefixes: perry_runtime::fs::|js_fs_
import { readFileSync } from "node:fs"; console.log(readFileSync("value.json","utf8").trim());
