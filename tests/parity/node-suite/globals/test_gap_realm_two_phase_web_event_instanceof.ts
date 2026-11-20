class CoreFirst {}
const core = [CoreFirst];
console.log(core.map(c => c.name).join(","));
console.log({} instanceof Event);
const ctor = Event;
const value = Object.create(ctor.prototype);
console.log(value instanceof ctor);
