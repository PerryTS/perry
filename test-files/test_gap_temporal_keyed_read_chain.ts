// A by-name read on a Temporal value is the ordinary [[Get]]: own expandos,
// then its built-in getters and methods, then the rest of its prototype
// chain (`Temporal.<Kind>.prototype`, then Object.prototype).
function read(o: any, k: string): any { return o[k]; }

const duration: any = new Temporal.Duration(1, 2, 3, 4);
const date: any = new Temporal.PlainDate(2020, 5, 6);
console.log("constructor", read(duration, "constructor") === Temporal.Duration,
  read(date, "constructor") === Temporal.PlainDate);
console.log("object proto", typeof read(duration, "hasOwnProperty"),
  typeof read(date, "isPrototypeOf"), read(duration, "missing"));
console.log("has own", read(duration, "hasOwnProperty").call(duration, "years"));

// A member a program adds to the kind prototype is found, with the cell as receiver.
(Temporal.Duration.prototype as any).describeYears = function (this: any) { return "y=" + this.years; };
Object.defineProperty(Temporal.Duration.prototype, "twiceDays", {
  get(this: any) { return this.days * 2; }, configurable: true,
});
console.log("user member", read(duration, "describeYears").call(duration), read(duration, "twiceDays"));
console.log("other kind", read(date, "twiceDays"), typeof read(date, "describeYears"));

// Own expandos still shadow the chain; the built-in getters still answer.
duration.constructor = "own";
console.log("own shadow", read(duration, "constructor"), read(duration, "years"), read(duration, "months"));
console.log("toString", typeof read(date, "toString"), String(read(date, "toString").call(date)));
