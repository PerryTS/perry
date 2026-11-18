class CoreFirst {}
const core = [CoreFirst];
console.log(core.map(c => c.name).join(","));
console.log(Object.getOwnPropertyNames(MessageEvent).join(","));
console.log(Object.getOwnPropertyNames(MessageEvent.prototype).join(","));
