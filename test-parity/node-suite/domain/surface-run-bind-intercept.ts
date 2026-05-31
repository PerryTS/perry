import domain from "node:domain";

console.log("keys:", Object.keys(domain).join(","));
console.log("exports:", typeof domain.Domain, typeof domain.createDomain, typeof domain.create);

const d = domain.createDomain();
console.log(
  "domain methods:",
  typeof d.run,
  typeof d.bind,
  typeof d.intercept,
  typeof d.add,
  typeof d.remove,
  typeof d.enter,
  typeof d.exit,
);
console.log("members array:", Array.isArray(d.members), d.members.length);
console.log("active initial:", String(domain.active));

d.on("error", (err: any) => {
  console.log(
    "domain error:",
    err.message,
    err.domain === d,
    err.domainThrown,
    typeof err.domainBound,
    typeof err.domainEmitter,
  );
});

d.run(
  function (a: string, b: string) {
    console.log("run active:", domain.active === d, a, b);
  },
  "a",
  "b",
);
console.log("active after run:", String(domain.active));

const intercepted = d.intercept((value: string, second: string) => {
  console.log("intercept callback:", value, second, domain.active === d);
});
console.log("intercept typeof:", typeof intercepted);
intercepted(null, "ok", "two");
console.log("after intercept success");
intercepted(new Error("first arg boom"), "ignored");
console.log("after intercept error");

const bound = d.bind((a: string, b: string) => {
  console.log("bound callback:", a, b, domain.active === d);
  throw new Error("bound boom");
});
console.log("bound typeof:", typeof bound);
bound("x", "y");
