// HWS hook: js_runtime_install_global_json
// HWS prefixes: perry_runtime::json::|js_json_|perry_runtime::object::global_this::json_reflect_namespaces::create_json_namespace|perry_runtime::object::global_this::bigint_promise::json_
// HWS companion: global_json
// HWS argv: ["json"]
// HWS node: bun_file_json.node.mjs
Bun.file("value.json")["json"]().then(value => console.log(value.answer));
