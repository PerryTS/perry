import domain from "node:domain";

const d = domain.create();
d.run(() =>
  setTimeout(
    () => console.log(domain.active === d, (process as any).domain === d),
    0,
  )
);
setTimeout(
  () => console.log(String(domain.active), String((process as any).domain)),
  0,
);
