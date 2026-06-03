import domain from "node:domain";

const d1 = domain.create();
const d2 = domain.create();

function domainLabel(value: any): string {
  if (value === d1) return "d1";
  if (value === d2) return "d2";
  return String(value);
}

function logState(label: string) {
  console.log(label, domainLabel(domain.active), domainLabel((process as any).domain));
}

logState("initial:");
console.log("enter d1 return:", String(d1.enter()));
logState("after enter d1:");
d2.enter();
logState("after enter d2:");
console.log("exit d2 return:", String(d2.exit()));
logState("after exit d2:");
d2.enter();
logState("after reenter d2:");
d1.exit();
logState("after exit d1:");
d2.exit();
logState("after duplicate exit d2:");
