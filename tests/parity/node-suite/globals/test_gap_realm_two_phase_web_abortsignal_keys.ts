class CoreFirst {}
const core = [CoreFirst];
console.log(core.map(c => c.name).join(","));
console.log(Object.getOwnPropertyNames(AbortSignal).join(","));
console.log(Object.getOwnPropertyNames(AbortSignal.prototype).join(","));
