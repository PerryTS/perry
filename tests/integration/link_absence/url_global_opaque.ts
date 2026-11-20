// HWS companion: global_url
// HWS hook: js_runtime_install_global_url
// HWS prefixes: perry_runtime::object::global_this::url|js_url_|perry_runtime::object::global_this::proto_methods::web_url_proto_methods|perry_runtime::object::class_registry::construct::builtin::web_url_construct|perry_runtime::object::global_this::install_static::web_url_statics
// HWS argv: ["URL"]
const host: any = globalThis;
const Constructor: any = host[process.argv[2]];
const value: any = new Constructor("https://example.com/a");
value.href = "https://example.org/b?x=1";
console.log(value.href, value.pathname, value.searchParams.get("x"));
