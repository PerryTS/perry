// HWS absent: js_nm_install_fs|js_node_submod_install_fs_promises|perry_runtime::node_stream
import { readFile } from "node:fs/promises";
const read = readFile;
console.log((await read("value.json", "utf8")).trim());
