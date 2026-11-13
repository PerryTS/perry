// A member a program puts on the prototype of a constructor a host object
// holds (`Temporal.Duration`, `Intl.NumberFormat`) is found by a read on an
// instance, and its body runs. In the whole-program solve such a
// constructor is a native function value whose own keys (`prototype`, its
// statics) the realm table did not list: `F.prototype` read as absent, the
// store or definition through it had no receiver, and the member's body was
// called inactive although the read below enters it (check mode: "inactive
// body: body entered", as on test_gap_temporal_keyed_read_chain.ts;
// `PERRY_FACTS_REACH_HELD_KEYS=0` restores that).
function read(o: any, k: string): any { return o[k]; }
const ran: string[] = [];

// A method, read as a value and called.
const duration: any = new Temporal.Duration(1, 2, 3, 4);
(Temporal.Duration.prototype as any).describeYears = function (this: any) {
  ran.push("describeYears");
  return "y=" + this.years;
};
console.log("method", read(duration, "describeYears").call(duration));
const held = duration.describeYears;
console.log("held", typeof held, held.call(new Temporal.Duration(9)));

// A getter, by a computed and by a named key.
Object.defineProperty(Temporal.Duration.prototype, "twiceDays", {
  get(this: any) { ran.push("twiceDays"); return this.days * 2; }, configurable: true,
});
console.log("getter", read(duration, "twiceDays"), duration.twiceDays);

// An instance a static made, a prototype held in an object first, and a
// definition made by a helper.
const date: any = Temporal.PlainDate.from("2020-05-06");
const reg: any = { p: Temporal.PlainDate.prototype };
Object.defineProperty(reg.p, "dayMonth", {
  get(this: any) { ran.push("dayMonth"); return this.day + "/" + this.month; }, configurable: true,
});
console.log("static instance", read(date, "dayMonth"), date.dayMonth);
function install(proto: any, name: string, fn: any): void {
  Object.defineProperty(proto, name, { get: fn, configurable: true });
}
install(Temporal.PlainTime.prototype, "hm", function (this: any) { ran.push("hm"); return this.hour * 100 + this.minute; });
const time: any = new Temporal.PlainTime(5, 6);
console.log("helper", read(time, "hm"));
const instant: any = Temporal.Instant.fromEpochMilliseconds(1000);
(Temporal.Instant.prototype as any).seconds = function (this: any) { ran.push("seconds"); return this.epochMilliseconds / 1000; };
console.log("instant", read(instant, "seconds").call(instant));

// Intl constructors are held the same way.
const nf: any = new Intl.NumberFormat("en-US");
(Intl.NumberFormat.prototype as any).one = function (this: any) { ran.push("one"); return this.format(1); };
console.log("intl method", read(nf, "one").call(nf), nf.one());
const dtf: any = new Intl.DateTimeFormat("en-US");
Object.defineProperty(Intl.DateTimeFormat.prototype, "kind", {
  get(this: any) { ran.push("kind"); return typeof this.format; }, configurable: true,
});
console.log("intl getter", read(dtf, "kind"), dtf.kind);
const collator: any = new Intl.Collator("en");
Object.defineProperty(Intl.Collator.prototype, "order", {
  get(this: any) { ran.push("order"); return this.compare("a", "b"); }, configurable: true,
});
console.log("intl collator", read(collator, "order"));

// The declared keys still answer.
console.log("statics", typeof Temporal.Duration.from, typeof Temporal.Duration.compare,
  typeof Intl.NumberFormat.supportedLocalesOf, typeof Temporal.Duration.prototype,
  (Temporal.Duration.prototype as any).constructor === Temporal.Duration,
  Temporal.Duration.compare(new Temporal.Duration(0, 0, 0, 1), new Temporal.Duration(0, 0, 0, 2)));
const from: any = Temporal.PlainDate.from;
console.log("static value", from.call(Temporal.PlainDate, "2021-02-03").day);

// Negative controls: a member of one kind is not another kind's; an own
// property shadows the chain; a function no one installs never runs; a
// redefined getter runs the last definition only.
console.log("other kind", read(date, "twiceDays"), typeof read(date, "describeYears"), read(nf, "kind"));
duration.twiceDays2 = "own";
console.log("own", read(duration, "twiceDays2"), read(duration, "years"));
function never(this: any): string { ran.push("never"); return "never"; }
console.log("never", typeof never, read(duration, "never"));
Object.defineProperty(Temporal.Duration.prototype, "twiceDays", {
  get(this: any) { ran.push("thriceDays"); return this.days * 3; }, configurable: true,
});
console.log("redefined", read(duration, "twiceDays"));
console.log("ran", ran.join(","));
