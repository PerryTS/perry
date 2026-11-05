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

// This positive witness also exercises the complete module installer.
import * as moduleSurface from "node:events";
if (Object.keys(moduleSurface).length === 0) throw new Error("empty builtin namespace");
