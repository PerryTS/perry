class CoreFirst {}
const core = [CoreFirst];
console.log(core.map(c => c.name).join(","));
console.log(Object.getOwnPropertyNames(Response).join(","));
console.log(Object.getOwnPropertyNames(Response.prototype).join(","));
