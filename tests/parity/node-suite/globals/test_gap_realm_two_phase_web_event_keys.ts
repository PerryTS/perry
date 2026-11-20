class CoreFirst {}
const core = [CoreFirst];
console.log(core.map(c => c.name).join(","));
console.log(Object.getOwnPropertyNames(Event).join(","));
console.log(Object.getOwnPropertyNames(Event.prototype).join(","));
