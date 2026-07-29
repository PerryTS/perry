import domain from "node:domain";

const d = domain.create();
d.on("error", (error: any) => {
  console.log(typeof error, error);
  console.log(String(domain.active), (domain as any)._stack.length);
});
d.run(() => {
  throw 42;
});
