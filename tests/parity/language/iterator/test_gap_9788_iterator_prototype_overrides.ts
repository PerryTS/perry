// #9788: declaration/expression and module/function loop paths must all read
// the current Symbol.iterator, including prototype accessors and inheritance.
class Range {
  name = "range";
  *[Symbol.iterator]() { yield 1; yield 2; }
}
function functionLoop() {
  const result: unknown[] = [];
  for (const value of new Range()) result.push(value);
  return result.join(",");
}
console.log("before", functionLoop());
Range.prototype[Symbol.iterator] = function* () { yield 7; yield 8; };
const moduleValues: unknown[] = [];
for (const value of new Range()) moduleValues.push(value);
console.log("loops", moduleValues.join(","), functionLoop());
console.log("call", new Range()[Symbol.iterator]().next().value);
console.log("array-from", Array.from(new Range()).join(","));

class Inherited extends Range {}
class Own extends Range { *[Symbol.iterator]() { yield 3; } }
console.log("inherit", [...new Inherited()].join(","), [...new Own()].join(","));
Inherited.prototype[Symbol.iterator] = function* () { yield 4; };
console.log("sub-override", [...new Inherited()].join(","), [...new Range()].join(","));

let accessorReceiver: any;
let gets = 0;
Object.defineProperty(Range.prototype, Symbol.iterator, {
  configurable: true,
  get() {
    gets++;
    accessorReceiver = this;
    return function* () { yield this.name; };
  },
});
const instance = new Range();
console.log("getter", [...instance].join(","), gets, accessorReceiver === instance);
Object.defineProperty(Range.prototype, Symbol.iterator, { value: undefined, configurable: true });
try { console.log([...instance]); } catch (error) { console.log("undefined", error instanceof TypeError); }

const Expression = class { *[Symbol.iterator]() { yield "old"; } };
Expression.prototype[Symbol.iterator] = function* () { yield "new"; };
console.log("expression", [...new Expression()].join(","));
console.log("expression-names", Object.getOwnPropertyNames(Expression.prototype).join(","));

class Plain {
  [Symbol.iterator]() { return [5, 6][Symbol.iterator](); }
}
console.log("non-generator", [...new Plain()].join(","));
console.log("plain-names", Object.getOwnPropertyNames(Plain.prototype).join(","));
class Literal {
  "@@iterator"() { return "literal"; }
}
console.log("literal", Object.getOwnPropertyNames(Literal.prototype).join(","), new Literal()["@@iterator"]());
const ownGetter: any = new Own();
let ownGets = 0;
Object.defineProperty(ownGetter, Symbol.iterator, {
  get() { ownGets++; return function* () { yield 9; }; },
});
console.log("own-getter-call", ownGetter[Symbol.iterator]().next().value, ownGets);
