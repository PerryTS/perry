// HWS companion: global_json
// HWS hook: js_runtime_install_global_json
// HWS prefixes: perry_runtime::json::|js_json_|perry_runtime::object::global_this::json_reflect_namespaces::create_json_namespace|perry_runtime::object::global_this::bigint_promise::json_
// HWS argv: ["JSON"]
const host: any = globalThis;
const J: any = host[process.argv[2]];
console.log("%j", {a: 1});
console.log(J.stringify({b: 2}));
