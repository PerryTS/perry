import domain from "node:domain";

const d = domain.create();
d.run(() =>
  setImmediate(() =>
    console.log(domain.active === d, (process as any).domain === d)
  )
);
setImmediate(() =>
  console.log(String(domain.active), String((process as any).domain))
);
