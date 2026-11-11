// HWS absent: js_nm_install_stream|js_runtime_install_os|perry_runtime::node_stream::constructors::builders::ensure_lazy_stream|perry_runtime::node_stream::constructors::builders::init_transform_kind|perry_runtime::node_stream::constructors::builders::init_duplex_in_place|perry_runtime::stream_hooks::ensure_lazy_stream|perry_runtime::stream_hooks::is_classic_stream_instance_of|perry_runtime::node_stream::readwrite::is_classic_stream_instance_of|js_process_stdin|js_native_export_73747265616d_
import { EventEmitter } from "node:events";
const emitter = new EventEmitter();
let total = 0;
emitter.on("value", (value: number) => { total += value; });
emitter.emit("value", 7);
console.log(total);
