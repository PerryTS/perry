import domain from "node:domain";

const d = domain.create();
d.on("error", (error: any) => {
  console.log(error.message, error.domain === d);
  console.log(error.domainThrown, error.domainEmitter, error.domainBound);
  console.log(String(domain.active), (domain as any)._stack.length);
});
d.run(() => {
  throw new Error("run");
});
