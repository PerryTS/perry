// A default import of a node builtin gives the module.exports value (the CJS value), not a namespace:
// stream -> Stream, events -> EventEmitter, module -> Module (node 26.8.1).
import sd from "node:stream";
import * as sn from "node:stream";
import ed from "node:events";
import * as en from "node:events";
import md from "node:module";
import * as mn from "node:module";

console.log("stream", typeof sd, sd === sn.Stream, sd === sn.default, sn.default === sn.Stream, sd.name);
console.log("events", typeof ed, ed === en.EventEmitter, ed === en.default, en.default === en.EventEmitter, ed.name);
console.log("module", typeof md, md === mn.Module, md === mn.default, mn.default === mn.Module, md.name);
