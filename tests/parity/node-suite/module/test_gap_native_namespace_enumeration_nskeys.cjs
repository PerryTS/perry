// Native CommonJS exports must survive enumeration-based interop.
// Compare sorted sets because namespace key order can differ.
function importStar(mod) {
  const result = {};
  for (const key in mod) {
    if (key !== "default" && Object.prototype.hasOwnProperty.call(mod, key)) {
      result[key] = mod[key];
    }
  }
  result.default = mod;
  return result;
}
function report(name, mod, spot) {
  console.log(name, "keys", Object.keys(mod).sort().join(","));
  const iterated = [];
  for (const key in mod) iterated.push(key);
  console.log(name, "for-in", iterated.sort().join(","));
  const entries = Object.entries(mod);
  const entryKeys = [];
  for (const entry of entries) entryKeys.push(entry[0]);
  console.log(name, "entries", entryKeys.sort().join(","));
  const copied = importStar(mod);
  const copyKeys = Object.keys(copied).filter(key => key !== "default");
  console.log(name, "importStar", copyKeys.sort().join(","));
  const spread = { ...mod };
  console.log(name, "spread", Object.keys(spread).sort().join(","));
  console.log(name, "assign", Object.keys(Object.assign({}, mod)).sort().join(","));
  console.log(name, "export", typeof mod[spot], typeof copied[spot], typeof spread[spot]);
}
const zlib = require("zlib");
report("zlib", zlib, "Gunzip");
report("fs", require("fs"), "readFileSync");
report("path", require("path"), "join");
report("events", require("events"), "EventEmitter");
report("crypto", require("crypto"), "createHash");
// minizlib selects the compression constructor from the interop copy.
const realZlib = importStar(zlib);
const mode = "Gunzip";
const Compression = realZlib[mode];
console.log("compression", typeof Compression);
if (typeof Compression === "function") {
  const stream = new Compression();
  console.log("stream", typeof stream.write, typeof stream.close);
  stream.close();
}
// An ordinary object's first value must never select a builtin's exports.
console.log("ordinary", Object.getOwnPropertyNames({ name: "zlib", tail: 1 }).sort().join(","));
const customRequire = require("module").createRequire(__filename);
console.log("createRequire", Object.keys(customRequire("node:zlib")).sort().join(","));
console.log("getBuiltinModule", Object.keys(process.getBuiltinModule("zlib")).sort().join(","));
console.log("alias", Object.keys(require("node:zlib")).sort().join(","));
// The callable-source path must perform [[Get]] and recheck later keys.
function source() {}
Object.defineProperty(source, "first", {
  enumerable: true,
  get() { delete source.tail; return "value"; }
});
source.tail = "removed";
const result = { ...source };
console.log("callable getter", Object.keys(result).join(","), result.first);
console.log("events accessors", typeof ({ ...require("events") }).EventEmitterAsyncResource,
  ({ ...require("events") }).defaultMaxListeners);
