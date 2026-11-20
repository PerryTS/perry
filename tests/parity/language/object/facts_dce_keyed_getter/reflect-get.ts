function p(a) { const o = {0: a}; Object.defineProperty(o, 0, { enumerable: false }); Object.defineProperty(o, 0, { get() { return 47; } }); return Reflect.get(o, 0); }
console.log("reflect", p(1));
