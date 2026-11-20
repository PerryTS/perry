// Preserve the entry filename so both runtimes use CommonJS.
console.log("bindings", typeof module, typeof require, typeof exports);
const mod = module;
const exposed = exports;
const load = require;
console.log("exports", mod.exports === exposed);
console.log("main", load.main === mod);
function sloppy() { return this === globalThis; }
console.log("sloppy", sloppy.call(undefined));
