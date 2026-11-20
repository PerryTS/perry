// HWS hook: js_runtime_install_global_webfetch
// HWS prefixes: perry_runtime::object::global_this::webfetch|perry_runtime::fetch::|js_response_|perry_runtime::object::global_this::proto_methods::web_webfetch_proto_methods|perry_runtime::object::class_registry::construct::builtin::web_webfetch_construct|perry_runtime::object::global_this::install_static::web_webfetch_statics
const R = globalThis.Response; console.log(new R("hi", {status:201}).status);
