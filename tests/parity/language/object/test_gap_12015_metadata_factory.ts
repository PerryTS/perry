function make(seed: number): any {
  return { value: seed, read() { return seed; }, extra: seed + 1 };
}
function inspect(receiver: any): void {
  const method = receiver.read;
  console.log(receiver.read(), method(), Object.keys(receiver).join(","));
  console.log(Object.values(receiver).map((v: any) => typeof v === "function" ? "fn" : v).join(","));
  console.log(Object.entries(receiver).map(([k, v]: any) => k + ":" + (typeof v === "function" ? "fn" : v)).join(","));
}
const first = make(11);
const second = make(22);
inspect(first);
inspect(second);
first.read = () => 99;
inspect(first);
inspect(second);
Object.defineProperty(second, "read", { get() { return () => 77; }, enumerable: true, configurable: true });
inspect(second);
delete first.extra;
console.log(Object.getOwnPropertyNames(first).join(","));
console.log(Object.assign({}, first).read());
console.log(({ ...second }).read());
