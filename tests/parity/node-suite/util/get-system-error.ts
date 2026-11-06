// util.getSystemErrorName / getSystemErrorMessage / getSystemErrorMap (#2514).
// Codes are libuv-style negatives; messages are libuv's (not libc strerror).
import util from "node:util";

console.log("name -2:", util.getSystemErrorName(-2));
console.log("name -13:", util.getSystemErrorName(-13));
console.log("name -4095:", util.getSystemErrorName(-4095));
console.log("name unmapped:", util.getSystemErrorName(-999999));
console.log("msg -2:", util.getSystemErrorMessage(-2));
console.log("msg -17:", util.getSystemErrorMessage(-17));
console.log("msg -21:", util.getSystemErrorMessage(-21));
console.log("msg -3008:", util.getSystemErrorMessage(-3008));
const m = util.getSystemErrorMap();
console.log("map isMap:", m instanceof Map);
console.log("map size:", m.size);
console.log("map -2:", JSON.stringify(m.get(-2)));
console.log("map -9:", JSON.stringify(m.get(-9)));
console.log("map -4094:", JSON.stringify(m.get(-4094)));
console.log("map has -4028:", m.has(-4028));
console.log("map has -4023:", m.has(-4023));
console.log("map has -4030:", m.has(-4030));
console.log("map has -4056:", m.has(-4056));

import { getSystemErrorName } from "node:util";
console.log("named -28:", getSystemErrorName(-28));

function probe(fnName, fn, value, label) {
  try {
    console.log("probe", fnName, label, "=>", JSON.stringify(fn(value)));
  } catch (e) {
    console.log(
      "probe",
      fnName,
      label,
      "THROW",
      e.name,
      e.code,
      JSON.stringify(e.message.split("\n")[0]),
    );
  }
}

for (const [fnName, fn] of [
  ["getSystemErrorName", util.getSystemErrorName],
  ["getSystemErrorMessage", util.getSystemErrorMessage],
]) {
  probe(fnName, fn, undefined, "undefined");
  probe(fnName, fn, null, "null");
  probe(fnName, fn, "-2", "string");
  probe(fnName, fn, -2, "valid");
  probe(fnName, fn, -2.1, "fraction");
  probe(fnName, fn, 2, "positive");
  probe(fnName, fn, 0, "zero");
  probe(fnName, fn, Infinity, "infinity");
  probe(fnName, fn, NaN, "nan");
  probe(fnName, fn, Number.MIN_SAFE_INTEGER - 1, "unsafe");
  probe(fnName, fn, 1n, "bigint");
  probe(fnName, fn, {}, "object");
  probe(fnName, fn, true, "boolean");
}
