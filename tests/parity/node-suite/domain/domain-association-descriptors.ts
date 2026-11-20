import domain from "node:domain";

const first = domain.create();
const second = domain.create();
const descriptor = Object.getOwnPropertyDescriptor(first, "domain") || {};
console.log(Object.hasOwn(first, "domain"), descriptor.value === null);
console.log(descriptor.writable, descriptor.enumerable, descriptor.configurable);
const names = Object.getOwnPropertyNames(first);
console.log(names.indexOf("domain") === 0, names.indexOf("domain") < names.indexOf("_events"));
console.log(!Object.keys(first).includes("domain"));
console.log(first !== second, first.members !== second.members);

const member: any = { domain: "sentinel" };
const before = Object.getOwnPropertyDescriptor(member, "domain")!;
first.remove(member);
const after = Object.getOwnPropertyDescriptor(member, "domain")!;
console.log(member.domain === null, first.members.length);
console.log(after.writable === before.writable, after.enumerable === before.enumerable,
  after.configurable === before.configurable);
