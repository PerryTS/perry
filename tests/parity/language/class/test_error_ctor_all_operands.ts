// every operand of an Error constructor call is evaluated, left to right,
// before the constructor runs (extra operands and spreads included).
const log: string[] = [];
const f = (t: string, v: unknown) => { log.push(t); return v; };

const e1 = new Error("m", undefined, f("x1", 1));
console.log(e1.message, log.join());
log.length = 0;

const e2 = new TypeError(f("a", "msg"), f("b", { cause: "c" }), f("c", 1), f("d", 2));
console.log(e2.message, (e2 as any).cause, log.join());
log.length = 0;

const e3 = new RangeError(f("a", "r"), undefined, f("c", 1));
console.log(e3.message, log.join());
log.length = 0;

const e4 = new AggregateError(f("e", [1, 2]), f("m", "agg"), f("o", { cause: 7 }), f("x", 1), f("y", 2));
console.log(e4.message, e4.errors.length, (e4 as any).cause, log.join());
log.length = 0;

// the message is converted after all operands are evaluated
const order: string[] = [];
const msg = { toString() { order.push("toString"); return "ts"; } };
const e5 = new Error(msg as any, undefined, (order.push("extra"), 1));
console.log(e5.message, order.join());

// spread operands
const args: any[] = ["sp", { cause: "k" }];
const e6 = new Error(...args);
console.log(e6.message, (e6 as any).cause);
const e7 = new SyntaxError(...(["only"] as any[]));
console.log(e7.message, (e7 as any).cause === undefined);
const e8 = new AggregateError(...([[3], "am"] as any[]));
console.log(e8.message, e8.errors.length);
