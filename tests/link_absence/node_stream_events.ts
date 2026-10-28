// HWS companion: node_stream
// HWS hook: js_nm_install_events
// HWS prefixes: perry_runtime::node_stream|js_node_stream_
import { EventEmitterAsyncResource } from "node:events";
class Emitter extends EventEmitterAsyncResource {
  constructor() { super({name: "witness"}); }
}
const emitter = new Emitter();
emitter.on("value", (value) => console.log(value));
emitter.emit("value", "ok");
console.log(emitter.asyncId > 0, emitter.asyncResource.eventEmitter === emitter);
emitter.emitDestroy();
