// HWS absent: js_runtime_install_os|js_process_stdin|js_native_export_73747265616d_
import { EventEmitter } from "node:events";
const emitter = new EventEmitter();
emitter.on("value", (value: number) => console.log(value));
emitter.emit("value", 7);
