class CoreFirst {}
const core = [CoreFirst];
console.log(core.map(c => c.name).join(","));
const ctor = Event;
console.log(ctor.name, typeof ctor);
console.log(Object.getPrototypeOf(ctor.prototype) === Object.prototype);
