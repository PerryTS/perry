function p(a) { const o = {k: a}; Object.defineProperty(o, "k", { enumerable: false }); Object.defineProperty(o, "k", { get() { return 42; } }); return o.k; }
console.log("named", p(1));
