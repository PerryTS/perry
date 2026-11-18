// Relational comparison of a class instance with a number: ToPrimitive runs
// @@toPrimitive / valueOf / toString as the chain defines them.
class Plain { x = 1; }
class WithValueOf { valueOf() { return 5; } }
class WithToString { toString() { return "3"; } }
class WithToPrimitive { [Symbol.toPrimitive](hint: string) { return hint === "number" ? 7 : 0; } }
class Tagged { get [Symbol.toStringTag]() { log.push("tag"); return "T"; } }
const log: string[] = [];
const cases: [string, any][] = [
  ["plain", new Plain()],
  ["valueOf", new WithValueOf()],
  ["toString", new WithToString()],
  ["toPrimitive", new WithToPrimitive()],
  ["tagged", new Tagged()],
];
for (const [name, o] of cases) {
  console.log(name, o < 4, o <= 4, o > 4, o >= 4, 4 < o, 4 >= o, o <= 0 || o > 0);
}
console.log("tag reads", log.length);
const own: any = new Plain();
own.valueOf = () => 9;
console.log("own valueOf", own > 8, own < 8);
class Sub extends WithValueOf {}
console.log("inherited", (new Sub() as any) > 4);
console.log("strings", (new Plain() as any) < "[object P", (new Plain() as any) > "[object A");
