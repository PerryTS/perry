// HWS absent: js_nm_install_util
import { types } from "node:util";
const key = "isUint8Array";
console.log(types[key](new Uint8Array(2)), types[key]([1, 2]));
