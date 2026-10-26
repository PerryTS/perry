// HWS companion: global_json
// HWS hook: js_runtime_install_global_json
// HWS prefixes: perry_runtime::json::|js_json_|perry_runtime::object::global_this::json_reflect_namespaces::create_json_namespace|perry_runtime::object::global_this::bigint_promise::json_
const print = console.log;
const pattern = process.argv[2] === "world" ? "%" + "j" : "x";
print(pattern, {d: 4});
