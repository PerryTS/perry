// HWS absent: js_runtime_install_os|js_process_stdin|js_native_export_73747265616d_|perry_runtime::node_stream::constructors::builders::ensure_lazy_stream|perry_runtime::node_stream::constructors::builders::init_transform_kind|perry_runtime::node_stream::constructors::builders::init_duplex_in_place|perry_runtime::stream_hooks::ensure_lazy_stream|perry_runtime::stream_hooks::is_classic_stream_instance_of|perry_runtime::node_stream::is_classic_stream_instance_of|perry_runtime::node_stream::readwrite::is_classic_stream_instance_of|perry_runtime::node_stream::proto_methods::ns_transform|perry_runtime::node_stream::proto_methods::ns_passthrough_transform
import { EventEmitter } from "node:events";
const emitter = new EventEmitter();
emitter.on("value", (value: number) => console.log(value));
emitter.emit("value", 7);
