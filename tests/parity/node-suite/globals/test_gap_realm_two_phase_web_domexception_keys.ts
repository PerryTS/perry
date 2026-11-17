class CoreFirst {}
const core = [CoreFirst];
console.log(core.map(c => c.name).join(","));
console.log(Object.getOwnPropertyNames(DOMException).join(","));
console.log(Object.getOwnPropertyNames(DOMException.prototype).join(","));
