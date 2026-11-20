const g: any = globalThis;

const globalCtor = g.EventTarget;
const bareCtor = EventTarget;

console.log("global typeof:", typeof globalCtor);
console.log("bare typeof:", typeof bareCtor);
console.log("same value:", globalCtor === bareCtor);
console.log("bare name/length:", EventTarget.name, EventTarget.length);
console.log("prototype type:", typeof globalCtor.prototype);

const target = new EventTarget();
console.log("instance type:", typeof target);
