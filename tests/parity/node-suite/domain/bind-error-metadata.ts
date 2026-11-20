import domain from "node:domain";

const d = domain.create();
const bound = d.bind(() => {
  throw new Error("bound");
});
d.on("error", (error: any) => {
  console.log(error.message, error.domain === d);
  console.log(error.domainThrown, error.domainEmitter, error.domainBound);
});
bound();
console.log(String(domain.active));
