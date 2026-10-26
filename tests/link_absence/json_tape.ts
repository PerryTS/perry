// HWS hook: js_runtime_install_global_json
// HWS prefixes: perry_runtime::json_tape::
// The JSON install owns the tape parser and its generic lazy-array slots.
// A >1KB array selects the existing auto tape route without an environment knob.
let text = "[";
for (let i = 0; i < 600; i++) text += (i ? "," : "") + i;
text += "]";
const values: any = JSON.parse(text);
console.log(values.length, values[509], values[2], values[509]);
values[700] = 42;
console.log(values.length, values[700], values[599], values[650] === undefined);
values.length = 3;
console.log(values.length, values[2], values[509] === undefined);
