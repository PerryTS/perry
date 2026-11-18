class CoreFirst {}
const core = [CoreFirst];
console.log(core.map(c => c.name).join(","));
console.log({} instanceof CustomEvent);
const ctor = CustomEvent;
const value = Object.create(ctor.prototype);
console.log(value instanceof ctor);
