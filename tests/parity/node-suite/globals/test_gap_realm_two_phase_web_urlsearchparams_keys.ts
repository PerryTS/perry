class CoreFirst {}
const core = [CoreFirst];
console.log(core.map(c => c.name).join(","));
console.log(Object.getOwnPropertyNames(URLSearchParams).join(","));
console.log(Object.getOwnPropertyNames(URLSearchParams.prototype).join(","));
