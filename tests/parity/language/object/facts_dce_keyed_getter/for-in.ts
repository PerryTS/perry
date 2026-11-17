function p(a) { const o = {0: a}; Object.defineProperty(o, 0, { value: 2 }); Object.defineProperty(o, 0, { get() { return 46; }, enumerable: true }); let n = 0; for (const k in o) n += o[k]; return n; }
console.log("for-in", p(1));
