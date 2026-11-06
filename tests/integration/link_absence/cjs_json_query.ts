// CJS 04-json-query: literal fs require must not select the loader.
// HWS argv: ["value.json"]
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
// Query a JSON file by dotted path: jq-lite.  e.g.  query data.json users.0.name
const fs_1 = require("fs");
function get(value, path) {
    let cur = value;
    for (const key of path) {
        if (cur === null || typeof cur !== "object")
            return undefined;
        cur = Array.isArray(cur) ? cur[Number(key)] : cur[key];
    }
    return cur;
}
function* walk(value, prefix = "") {
    if (value !== null && typeof value === "object") {
        for (const [k, v] of Object.entries(value)) {
            yield* walk(v, prefix ? `${prefix}.${k}` : k);
        }
    }
    else {
        yield [prefix, value];
    }
}
const [file, query] = process.argv.slice(2);
if (!file) {
    console.error("usage: query <file.json> [dotted.path | --paths]");
    process.exit(1);
}
const data = JSON.parse((0, fs_1.readFileSync)(file, "utf8"));
if (query === "--paths") {
    for (const [p, v] of walk(data))
        console.log(`${p} = ${JSON.stringify(v)}`);
}
else {
    const result = query ? get(data, query.split(".")) : data;
    console.log(result === undefined ? "undefined" : JSON.stringify(result, null, 2));
}
