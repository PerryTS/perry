function p() { const o = {}; Object.defineProperty(o, 0, { get() { return 11; }, configurable: true }); Object.defineProperty(o, 0, { value: 1, writable: true, configurable: true }); Object.defineProperty(o, 0, { get() { return 45; } }); return o[0]; }
console.log("define", p());
