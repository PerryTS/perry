// HWS hook: js_runtime_install_json_engine
// HWS prefixes: perry_runtime::json::|js_json_|perry_runtime::object::global_this::json_reflect_namespaces::create_json_namespace|perry_runtime::object::global_this::bigint_promise::json_
// HWS companion: global_json
// HWS node: bun_jsonl_json.node.mjs
console.log(Bun.JSONL.parseChunk('{"answer":42}\n').values[0].answer);
