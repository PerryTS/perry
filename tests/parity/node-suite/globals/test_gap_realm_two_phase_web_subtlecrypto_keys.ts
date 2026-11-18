class CoreFirst {}
const core = [CoreFirst];
console.log(core.map(c => c.name).join(","));
console.log(Object.getOwnPropertyNames(SubtleCrypto).join(","));
console.log(Object.getOwnPropertyNames(SubtleCrypto.prototype).join(","));
