import domain from "node:domain";

const d = domain.create();
console.log(
  String(d.enter()),
  domain.active === d,
  (process as any).domain === d,
);
console.log(
  String(d.exit()),
  String(domain.active),
  String((process as any).domain),
);
