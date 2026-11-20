class CoreFirst {}
const core = [CoreFirst];
console.log(core.map(c => c.name).join(","));
console.log(Object.getOwnPropertyNames(CustomEvent).join(","));
console.log(Object.getOwnPropertyNames(CustomEvent.prototype).join(","));
