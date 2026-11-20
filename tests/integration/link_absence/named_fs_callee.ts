// HWS absent: js_nm_install_fs|perry_runtime::node_stream
import { readFileSync } from "node:fs";
console.log(readFileSync("value.json", "utf8").trim());
