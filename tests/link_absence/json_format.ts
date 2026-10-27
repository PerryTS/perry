// HWS companion: global_json
// HWS hook: js_runtime_install_json_format
// HWS prefixes: perry_runtime::json::|js_json_|perry_runtime::object::global_this::json_reflect_namespaces::create_json_namespace|perry_runtime::object::global_this::bigint_promise::json_
import { format } from "node:util";
const cycle: any = {}; cycle.self = cycle;
console.log(format("%j", {a: 1}));
console.log(format("%j", cycle));
