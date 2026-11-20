// Known-symbol class members drive the protocols (node is the oracle).
class Money {
  cents: number;
  constructor(cents: number) { this.cents = cents; }
  [Symbol.toPrimitive](hint: string) { return hint === "number" ? this.cents / 100 : `$${this.cents / 100}`; }
}
const m = new Money(250);
console.log(+m, `${m}`, m + "", (m as any) * 2);
class Even {
  static [Symbol.hasInstance](x: unknown) { return typeof x === "number" && x % 2 === 0; }
}
console.log(4 instanceof (Even as any), 3 instanceof (Even as any));
class Range {
  n: number;
  constructor(n: number) { this.n = n; }
  *[Symbol.iterator]() { for (let i = 0; i < this.n; i++) yield i; }
}
class Doubled extends Range {
  *[Symbol.iterator]() { for (const v of super[Symbol.iterator]()) yield v * 2; }
}
class Plain extends Range {}
let s = 0; for (const v of new Range(4)) s += v; console.log("range", s);
console.log("doubled", [...new Doubled(4)].join(","));
console.log("plain", [...new Plain(3)].join(","), Object.getOwnPropertySymbols(Plain.prototype).length);
console.log(Doubled.prototype[Symbol.iterator] !== Range.prototype[Symbol.iterator]);
// Reassign the prototype member later: every consumer sees the new value.
const r = new Range(3);
(Range.prototype as any)[Symbol.iterator] = function* () { yield "x"; yield "y"; };
console.log("reassigned", [...r].join(","), [...new Plain(5)].join(","), [...new Doubled(2)].join(","));
const [a, b] = r as any; console.log("destructure", a, b);
console.log("from", Array.from(r as any).join(","));
// Own instance property shadows the prototype member.
const own = new Range(2);
(own as any)[Symbol.iterator] = function* () { yield "own"; };
console.log("own", [...own].join(","));
delete (Range.prototype as any)[Symbol.iterator];
try { [...(new Range(1) as any)]; console.log("deleted: iterable"); } catch (e) { console.log("deleted:", (e as Error).constructor.name); }
// Symbol.for key method and a dispose member.
const k = Symbol.for("thr1.k");
class K { [k]() { return "k"; } [Symbol.dispose]() { console.log("disposed"); } }
console.log(new K()[k](), (K.prototype as any)[Symbol.for("thr1.k")] === K.prototype[k]);
{ using x = new K(); console.log("in block", typeof x); }
