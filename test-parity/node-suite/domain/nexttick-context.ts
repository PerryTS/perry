import domain from "node:domain";

const d = domain.create();
d.run(() => {
  process.nextTick(() =>
    console.log(domain.active === d, (process as any).domain === d)
  );
});
process.nextTick(() =>
  console.log(String(domain.active), String((process as any).domain))
);
