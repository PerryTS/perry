// #11928: compiled functions read exact retained source from their immutable
// JsFunctionInfo record; raw class members keep the compatibility registry.
function outer() {
  function nested() { return "世界"; }
  return nested;
}

const arrow = (value = "é") => value + "!";
async function asyncFunction() { return "async"; }
function* generatorFunction() { yield "generator"; }
async function* asyncGeneratorFunction() { yield "async-generator"; }

class Cafe {
  method() { return "méthode"; }
  get value() { return "välue"; }
}

const nested = outer();
const descriptor = Object.getOwnPropertyDescriptor(Cafe.prototype, "value")!;
const cases: Array<[string, Function]> = [
  ["outer", outer],
  ["nested", nested],
  ["arrow", arrow],
  ["async", asyncFunction],
  ["generator", generatorFunction],
  ["async-generator", asyncGeneratorFunction],
  ["class", Cafe],
  ["method", Cafe.prototype.method],
  ["getter", descriptor.get!],
];

for (const [name, fn] of cases) {
  console.log(name + ":" + JSON.stringify(Function.prototype.toString.call(fn)));
}
