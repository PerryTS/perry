// HWS absent: js_nm_install_events|perry_runtime::node_stream::|js_native_export_73747265616d_|js_nm_install_stream|perry_runtime::node_stream::|js_native_export_73747265616d_
import { EventEmitter } from "node:events";
const emitter = new EventEmitter();
let total = 0;
emitter.on("value", (value: number) => { total += value; });
emitter.emit("value", 7);
console.log(total);
