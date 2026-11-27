// `++`/`--` on a BigInt object field applies ToNumeric, so the field stays a
// BigInt whether or not the compiler keeps the object in locals.

function show(label: string, run: () => unknown): void {
  try {
    const v = run();
    console.log(label, typeof v, String(v));
  } catch (e) {
    console.log(label, (e as Error).constructor.name);
  }
}

show("local object postfix", () => {
  const o = { n: 1n };
  o.n++;
  return o.n;
});
show("local object prefix", () => {
  const o = { n: 1n };
  return ++o.n;
});
show("local object postfix value", () => {
  const o = { n: 5n };
  return o.n--;
});
show("any object", () => {
  const o: any = { n: 1n };
  o.n++;
  return o.n;
});
show("number field", () => {
  const o = { n: 1.5 };
  o.n++;
  return o.n;
});
show("string field", () => {
  const o: any = { n: "4" };
  o.n--;
  return o.n;
});
class Counter {
  n: bigint;
  constructor() {
    this.n = 1n;
    this.n++;
  }
}
show("constructor field", () => new Counter().n);
show("loop", () => {
  const o = { n: 0n };
  for (let i = 0; i < 3; i++) o.n++;
  return o.n;
});
