import domain from "node:domain";

const d = domain.create();
const member: any = { domain: "sentinel" };
d.remove(member);
console.log(member.domain === null, d.members.length);
