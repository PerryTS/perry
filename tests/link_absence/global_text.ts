// HWS hook: js_runtime_install_global_text
// HWS prefixes: perry_runtime::text_encoder::|perry_runtime::text_decoder::|js_text_encoder_|perry_runtime::object::global_this::proto_methods::web_text_proto_methods|perry_runtime::object::class_registry::construct::builtin::web_text_construct|perry_runtime::object::global_this::install_static::web_text_statics
const E = globalThis.TextEncoder; console.log(new E().encode("hi").length);
