// HWS hook: js_runtime_install_global_reflect
// HWS prefixes: perry_runtime::object::global_this::reflect_namespace::|perry_runtime::object::global_this::json_reflect_namespaces::create_reflect_namespace|perry_runtime::object::global_this::bigint_promise::reflect_
const r = globalThis.Reflect; console.log(r.ownKeys({a:1}).length);
