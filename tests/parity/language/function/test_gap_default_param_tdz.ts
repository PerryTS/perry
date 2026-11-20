function assert(cond: boolean, msg: string) {
  if (!cond) {
    throw new Error(msg);
  }
}

function assertReferenceError(fn: () => unknown, msg: string) {
  let threw = false;
  try {
    fn();
  } catch (err) {
    threw = err instanceof ReferenceError;
  }
  assert(threw, msg);
}

let outer = 2;

function laterParam(a = b, b = 1) {
  return a;
}

function laterParamShadowsOuter(a = outer, outer = 1) {
  return a;
}

const arrowLaterParam = (a = b, b = 1) => a;
const closureDefault = (a = () => b, b = 1) => a();

assertReferenceError(() => laterParam(), "function default must TDZ-read later param");
assertReferenceError(
  () => laterParamShadowsOuter(),
  "later param must shadow outer binding during default evaluation",
);
assertReferenceError(() => arrowLaterParam(), "arrow default must TDZ-read later param");
assert(closureDefault() === 1, "nested closure default should not TDZ-read early");

console.log("default param TDZ ok");
