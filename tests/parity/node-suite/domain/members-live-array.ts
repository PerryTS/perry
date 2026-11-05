import domain from "node:domain";

const d = domain.create();
const members = d.members;
members.push({} as any);
console.log(d.members === members, d.members.length, members.length);
members.length = 0;
