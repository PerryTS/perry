// HWS absent: js_nm_install_events
import { EventEmitter } from "node:events";
const emitter = new EventEmitter();
let total = 0;
emitter.on("value", (value: number) => { total += value; });
emitter.emit("value", 7);
console.log(total);
