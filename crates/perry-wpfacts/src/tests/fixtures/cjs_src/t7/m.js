const Reply = require("./a");
const poke = require("./b");
globalThis.keep = poke;
const r = new Reply(1);
console.log(r.f(), r.n);
