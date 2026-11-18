class CoreFirst {}
const core = [CoreFirst];
console.log(core.map(c => c.name).join(","));
console.log({} instanceof URL);
const ctor = URL;
const value = Object.create(ctor.prototype);
console.log(value instanceof ctor);
