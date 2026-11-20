// Issue #5894: computed property keys must stay visible through the same
// reflective surfaces as non-computed properties.

const sym1 = Symbol("one");
const sym2 = Symbol("two");

class C {
  ["constructor"](): number {
    return 1;
  }

  [sym1](): string {
    return "first";
  }

  [((value: symbol): symbol => value)(sym2)](): string {
    return "second";
  }
}

const instance = new C();
const prototypeSymbols = Object.getOwnPropertySymbols(C.prototype);
console.log(C === C.prototype.constructor);
console.log(
  Object.getOwnPropertyDescriptor(C.prototype, "constructor")?.value === C,
);
console.log(instance.constructor());
console.log(instance[sym1]());
console.log(instance[sym2]());
console.log(prototypeSymbols.length);
console.log(prototypeSymbols[0] === sym1);
console.log(prototypeSymbols[1] === sym2);

const numericKeys = {
  [1.2]: "finite",
  [-0]: "zero",
  [Infinity]: "positive infinity",
  [-Infinity]: "negative infinity",
  [NaN]: "not a number",
};

console.log(numericKeys[1.2]);
console.log(numericKeys[-0]);
console.log(numericKeys[Infinity]);
console.log(numericKeys[-Infinity]);
console.log(numericKeys[NaN]);
