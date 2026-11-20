// HWS hook: js_runtime_install_global_webcrypto
// HWS prefixes: perry_runtime::webcrypto::|js_webcrypto_|perry_runtime::object::global_this::proto_methods::web_webcrypto_proto_methods|perry_runtime::object::class_registry::construct::builtin::web_webcrypto_construct|perry_runtime::object::global_this::install_static::web_webcrypto_statics
const c = globalThis.crypto; console.log(typeof c.subtle);
