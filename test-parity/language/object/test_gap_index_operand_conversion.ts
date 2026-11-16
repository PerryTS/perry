// ECMA-262 23.1.3 / 23.2.3 / 23.1.2.1: the index, count and length operands of
// these builtins are converted with ToIntegerOrInfinity / ToLength (7.1.5, 7.1.20
// -> ToNumber -> ToPrimitive hint number): an object operand's `valueOf` runs,
// once per operand, in argument order. The unknown value comes from argv.
"use strict";
const V: number = Number(process.argv[2] || 1);
const log: string[] = [];
const mk = (n: string, v: number): any => ({
  valueOf() {
    log.push(n);
    return v;
  },
});
const arr = (): any => [1, 2, 3, 4, 5];
const u8 = (): any => new Uint8Array([1, 2, 3, 4, 5]);
const f64 = (): any => new Float64Array([1, 2, 3, 4, 5]);
const show = (name: string, f: () => any): void => {
  log.length = 0;
  let r: any;
  try {
    r = f();
  } catch (e: any) {
    r = "ERR " + e.name;
  }
  const text = r !== null && typeof r === "object" ? Array.from(r).join() : String(r);
  console.log(name, text, log.join(","));
};
show("at", () => arr().at(mk("i", V)));
show("with", () => arr().with(mk("i", V), 9));
show("toSpliced", () => arr().toSpliced(mk("s", V), mk("c", V)));
show("toSpliced+items", () => arr().toSpliced(mk("s", V), mk("c", V), 7, 8));
show("u8.subarray", () => u8().subarray(mk("b", V), mk("e", V + 2)));
show("f64.subarray", () => f64().subarray(mk("b", V), mk("e", V + 2)));
show("from length", () => Array.from({ length: mk("l", V + 1) }));
show("u8.at", () => u8().at(mk("i", V)));
show("f64.at", () => f64().at(mk("i", V)));
show("f64.with", () => f64().with(mk("i", V), 9));
show("u8.slice", () => u8().slice(mk("b", V), mk("e", V + 2)));
show("f64.slice", () => f64().slice(mk("b", V), mk("e", V + 2)));
show("u8.copyWithin", () => u8().copyWithin(mk("t", V), mk("s", V + 1), mk("e", V + 3)));
show("f64.copyWithin", () => f64().copyWithin(mk("t", V), mk("s", V + 1), mk("e", V + 3)));
