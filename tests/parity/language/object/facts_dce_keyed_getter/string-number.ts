function p(a) { const o = {0: a}; Object.defineProperty(o, "0", { enumerable: false }); Object.defineProperty(o, "0", { get() { return 43; } }); return o[0] + o["0"]; }
console.log("keys", p(1));
