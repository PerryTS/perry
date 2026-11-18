class CoreFirst {}
const core = [CoreFirst];
console.log(core.map(c => c.name).join(","));
const key = ["DOMException"].join("");
const ctor = globalThis[key];
console.log(ctor.name, typeof ctor);
console.log(Object.getOwnPropertyNames(ctor).join(","));
console.log(Object.getOwnPropertyNames(ctor.prototype).join(","));
