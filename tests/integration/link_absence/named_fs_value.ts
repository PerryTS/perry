// HWS absent: js_nm_install_fs|perry_runtime::node_stream
import { readFileSync } from "node:fs";
const f = readFileSync; console.log([f].map(read => read("value.json", "utf8").trim()).join("|"));
