import domain from "node:domain";

const d = domain.create();
const member: any = {};
d.add(member);
console.log(member.domain === d, d.members[0] === member);
console.log(Object.prototype.propertyIsEnumerable.call(member, "domain"));
d.remove(member);
