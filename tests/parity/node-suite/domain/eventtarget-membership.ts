import domain from "node:domain";

const d = domain.create();
const target: any = new EventTarget();
d.add(target);
console.log(target.domain === d, d.members[0] === target);
console.log(Object.prototype.propertyIsEnumerable.call(target, "domain"));
d.remove(target);
console.log(target.domain === null);
