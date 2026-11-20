function assertSame(label: string, actual: any, expected: any) {
  if (actual !== expected) {
    throw new Error(label + ": expected " + String(expected) + ", got " + String(actual));
  }
}

function assertTrue(label: string, actual: any) {
  if (!actual) {
    throw new Error(label + ": expected truthy, got " + String(actual));
  }
}

function joinThis(this: any, a: any, b: any) {
  return [this && this.tag, a, b].join("|");
}

assertSame("direct call", (joinThis as any).call({ tag: "C" }, 1, 2), "C|1|2");
assertSame("direct apply", (joinThis as any).apply({ tag: "A" }, [3, 4]), "A|3|4");

const callValue: any = Function.prototype.call;
const applyValue: any = Function.prototype.apply;
assertSame("value-read call.call", callValue.call(joinThis, { tag: "VC" }, 5, 6), "VC|5|6");
assertSame("value-read apply.call", applyValue.call(joinThis, { tag: "VA" }, [7, 8]), "VA|7|8");
assertSame(
  "value-read call.apply",
  callValue.apply(joinThis, [{ tag: "VCA" }, 9, 10]),
  "VCA|9|10",
);

const hop: any = Function.prototype.call.bind(Object.prototype.hasOwnProperty);
assertTrue("uncurry hit", hop({ x: 1 }, "x"));
assertSame("uncurry miss", hop({ x: 1 }, "y"), false);

const m = new Map();
const ret = Map.prototype.set.call(m, "k", 42);
assertSame("borrowed map set return", ret, m);
assertSame("borrowed map set value", m.get("k"), 42);

function strictEvalTypeofThis() {
  "use strict";
  return eval("typeof this");
}

function strictEvalThis() {
  "use strict";
  return eval("this");
}

assertSame("strict direct eval typeof this", strictEvalTypeofThis(), "undefined");
assertSame("strict direct eval this", strictEvalThis(), undefined);
assertSame("global direct eval this", eval("\"use strict\";\nthis"), this);

var myEval: any = eval;
function indirectEvalThis() {
  return myEval("\"use strict\";\nthis") === this;
}
assertTrue("indirect eval global this", indirectEvalThis());

console.log("issue-3579-ok");
