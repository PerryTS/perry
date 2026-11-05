const sym = Symbol("field");
const fn: any = () => 0;
const protos: any[] = [null, { inherited: 7 }, fn];
for (const proto of protos) {
  const desc: any = { value: { value: 11, writable: true, enumerable: true, configurable: true } };
  desc[sym] = { value: 13, enumerable: true, configurable: true };
  const empty: any = Object.create(proto);
  const obj: any = Object.create(proto, desc);
  console.log("proto", Object.getPrototypeOf(empty) === proto, Object.getPrototypeOf(obj) === proto);
  console.log("fields", obj.value, obj[sym], Object.getOwnPropertyNames(obj).join("|"),
    Object.getOwnPropertySymbols(obj).map(String).join("|"));
}
for (const bad of [undefined, 1, true, "x", Symbol("bad")]) {
  try { Object.create(bad); console.log("accepted"); }
  catch (e: any) { console.log("invalid", e instanceof TypeError); }
}
const proto: any = { inherited: 19 };
const log: string[] = [];
const bag: any = { get x() { log.push("descriptor"); return { value: 23, enumerable: true }; } };
console.log("bag", Object.create(proto, bag).x, log.join("|"));
