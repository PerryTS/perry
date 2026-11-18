class CoreFirst {}
const core = [CoreFirst];
console.log(core.map(c => c.name).join(","));
console.log(Object.getOwnPropertyNames(URL).join(","));
console.log(Object.getOwnPropertyNames(URL.prototype).join(","));
