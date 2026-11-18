function p() { const o = {get 0() { return 11; }}; Object.defineProperty(o, 0, { value: 1, writable: true, configurable: true }); Object.defineProperty(o, 0, { get() { return 44; } }); return o[0]; }
console.log("literal", p());
