import domain from "node:domain";

const d = domain.create();
d.run(() => {
  Promise.resolve().then(() =>
    console.log(domain.active === d, (process as any).domain === d)
  );
});
Promise.resolve().then(() =>
  console.log(String(domain.active), String((process as any).domain))
);
