function p4(a) { const o = {0: a}; Object.defineProperty(o, 0, { enumerable: false }); Object.defineProperty(o, 0, { get() { return 42; } }); return o[0]; }
console.log("p4", p4(1));
