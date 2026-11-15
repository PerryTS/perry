// An accessor a program installs on a built-in prototype runs for every
// receiver that inherits it, a primitive included. The whole-program solve
// holds every built-in prototype as one object and walks accessors only for
// the receivers it follows as objects; a primitive's read (and an unknown
// value's) ended at the described members, so the getter's body was called
// inactive although the read below enters it (check mode: "inactive body:
// body entered"; `PERRY_FACTS_REACH_PROTO_ACCESSORS=0` restores that).
// A program this size hands the global object to unknown code somewhere, which
// makes the prototypes unknown and the accessors escape on their own; the
// small programs that keep the hole open are string/test_reach_string_index_getter.ts
// and scripts/verify_facts_s0/reach-proto-accessor-negative.ts.
function read(o: any, k: string): any { return o[k]; }
const ran: string[] = [];

Object.defineProperty(String.prototype, "shout", {
  get(this: any) { ran.push("shout"); return String(this).toUpperCase() + "!"; }, configurable: true,
});
console.log("string", read("abc", "shout"), ("xy" as any).shout);

Object.defineProperty(Number.prototype, "half", {
  get(this: any) { ran.push("half"); return Number(this) / 2; }, configurable: true,
});
console.log("number", read(8, "half"), (5 as any).half);

Object.defineProperty(Boolean.prototype, "flip", {
  get(this: any) { ran.push("flip"); return !this.valueOf(); }, configurable: true,
});
console.log("boolean", read(true, "flip"), read(false, "flip"));

// Installed by a helper, and by Object.defineProperties.
function install(proto: any, name: string, fn: any): void {
  Object.defineProperty(proto, name, { get: fn, configurable: true });
}
install(String.prototype, "size2", function (this: any) { ran.push("size2"); return this.length * 2; });
console.log("helper", read("abcd", "size2"));
Object.defineProperties(Number.prototype, {
  neg: { get(this: any) { ran.push("neg"); return -Number(this); }, configurable: true },
});
console.log("defineProperties", read(3, "neg"));

// One read site, receivers of several kinds.
const values: any[] = ["s", 7, "t", 9];
const keys: string[] = ["shout", "half", "size2", "neg"];
let all = "";
for (let i = 0; i < values.length; i++) all += String(read(values[i], keys[i])) + " ";
console.log("mixed", all);

// An index key: a string index read past the end goes on to the chain.
function at(t: string, k: number): any { return t[k]; }
function atAny(t: any, k: number): any { return t[k]; }
Object.defineProperty(String.prototype, "5", {
  get(this: any) { ran.push("five"); return "<" + this.length + ">"; }, configurable: true,
});
let idx = "";
for (let k = 0; k < 7; k++) idx += at("abc", k) + atAny("xy", k) + ";";
console.log("index", idx, at("abcdefg", 5));

// Negative controls: a data property is no accessor; a kind without the
// member reads undefined; a getter no one installs never runs; a redefined
// getter runs the last definition only; a deleted one runs no more.
Object.defineProperty(String.prototype, "plain", { value: 41, configurable: true });
console.log("data", read("s", "plain"));
console.log("other kind", read(7, "shout"), read("s", "half"), read(true, "half"));
function never(this: any): string { ran.push("never"); return "never"; }
console.log("never", typeof never, read("s", "never"));
Object.defineProperty(String.prototype, "shout", {
  get(this: any) { ran.push("whisper"); return String(this).toLowerCase() + "?"; }, configurable: true,
});
console.log("redefined", read("ABC", "shout"));
delete (Number.prototype as any).half;
console.log("deleted", read(8, "half"));
console.log("ran", ran.join(","));
