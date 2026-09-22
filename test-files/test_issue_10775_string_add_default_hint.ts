// A folded + chain must use ToPrimitive(default), then ToString, at each Add.
// Template substitutions instead use ToString (the "string" hint).
const valueOfOnly: any = { valueOf: () => 5 };
console.log("a" + valueOfOnly + "b");

const both: any = { valueOf: () => 1, toString: () => "T" };
console.log("a" + both + "b");

const hints: string[] = [];
const exotic: any = {
  [Symbol.toPrimitive](hint: string) {
    hints.push(hint);
    return "P";
  },
};
console.log("a" + exotic + "b", hints.join(","));
console.log(`a${exotic}b`, hints.join(","));

const order: string[] = [];
function tracked(label: string): any {
  order.push("eval-" + label);
  return {
    [Symbol.toPrimitive](hint: string) {
      order.push("coerce-" + label + "-" + hint);
      return label;
    },
  };
}
console.log(tracked("A") + ":" + tracked("B") + ":");
console.log(order.join(","));

// The left operand's coercion may mutate an object already evaluated as the
// right operand of that first Add. Both values must be kept alive in order.
const right: any = { valueOf: () => 2 };
const left: any = {
  valueOf() {
    right.valueOf = () => 3;
    return 1;
  },
};
console.log(left + ":" + right + "x");

// An object may return a Symbol from ToPrimitive. The + operator throws;
// explicit String() and template substitutions have different rules.
const symbolObject: any = { [Symbol.toPrimitive]: () => Symbol("s") };
try {
  console.log("a" + symbolObject + "b");
} catch (error) {
  console.log(error instanceof TypeError);
}
