// HWS hook: js_runtime_install_global_url
// HWS prefixes: perry_runtime::object::global_this::url|js_url_|perry_runtime::object::global_this::proto_methods::web_url_proto_methods|perry_runtime::object::class_registry::construct::builtin::web_url_construct|perry_runtime::object::global_this::install_static::web_url_statics
const U = globalThis.URL; console.log(new U("https://example.com/a").pathname);
