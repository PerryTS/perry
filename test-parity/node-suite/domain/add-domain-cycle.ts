import domain from "node:domain";

const parent = domain.create();
const child = domain.create();
parent.add(child);
child.add(parent);
console.log(parent.members.includes(child), child.members.includes(parent));
console.log((child as any).domain === parent, (parent as any).domain === null);
parent.remove(child);
