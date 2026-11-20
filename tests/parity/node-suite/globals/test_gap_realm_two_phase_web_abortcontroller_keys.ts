class CoreFirst {}
const core = [CoreFirst];
console.log(core.map(c => c.name).join(","));
console.log(Object.getOwnPropertyNames(AbortController).join(","));
console.log(Object.getOwnPropertyNames(AbortController.prototype).join(","));
