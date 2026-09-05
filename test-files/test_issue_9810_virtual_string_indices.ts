// #9810: boxing must not eagerly allocate one property per UTF-16 code unit.
function check(ok: boolean, label: string): void {
  if (!ok) throw new Error(label);
}
function equal(actual: any, expected: any, label: string): void {
  check(JSON.stringify(actual) === JSON.stringify(expected), label);
}
function throws(fn: () => void, label: string): void {
  let threw = false;
  try { fn(); } catch (e) { threw = e instanceof TypeError; }
  check(threw, label);
}

const text = "a😀b";
const s: any = Object(text);
check(s.length === 4 && s.valueOf() === text, "payload and UTF-16 length");
for (let i = 0; i < 4; i++) {
  check(s[i] === text[i], "index read " + i);
  check(Object.hasOwn(s, String(i)) && s.hasOwnProperty(i) && i in s, "own index " + i);
  check(s.propertyIsEnumerable(i), "enumerable index " + i);
  equal(Object.getOwnPropertyDescriptor(s, String(i)), {
    value: text[i], writable: false, enumerable: true, configurable: false,
  }, "index descriptor " + i);
}
for (const key of ["-0", "-1", "01", "1.0", "1.5", "4", "NaN", "4294967295"]) {
  check(!Object.hasOwn(s, key), "absent " + key);
}
const symbol = Symbol("extra");
s[symbol] = 17;
s.extra = 9;
s[7] = "outside";
s["01"] = "leading";
Object.defineProperty(s, "hidden", { value: 10 });
equal(Object.keys(s), ["0", "1", "2", "3", "7", "extra", "01"], "keys order");
equal(Object.getOwnPropertyNames(s), ["0", "1", "2", "3", "7", "length", "extra", "01", "hidden"], "names order");
const ownKeys = Reflect.ownKeys(s);
check(ownKeys.length === 10 && ownKeys[9] === symbol, "symbol ordering");
equal(Object.values(s), [text[0], text[1], text[2], text[3], "outside", 9, "leading"], "values");
equal(Object.entries(Object("ab")), [["0", "a"], ["1", "b"]], "entries");
equal(Object.keys(Object.getOwnPropertyDescriptors(Object("ab"))), ["0", "1", "length"], "descriptors");
const loop: string[] = [];
for (const key in s) loop.push(key);
equal(loop, Object.keys(s), "for-in");
const assigned: any = Object.assign({}, s);
check(assigned[0] === "a" && assigned[3] === "b" && assigned.extra === 9 && assigned[symbol] === 17, "assign");
check(!Object.hasOwn(assigned, "length") && !Object.hasOwn(assigned, "hidden"), "assign filters");
const spread: any = { ...s, tail: 12 };
check(spread[0] === "a" && spread[3] === "b" && spread.tail === 12, "spread");
const { 0: first, ...rest } = s;
check(first === "a" && !Object.hasOwn(rest, "0") && rest[3] === "b", "rest");
check(JSON.stringify(s) === JSON.stringify(text), "JSON unwraps");

check(!Reflect.set(s, "0", "x"), "Reflect.set rejects");
check(!Reflect.deleteProperty(s, "0"), "Reflect.delete rejects");
check(!Reflect.defineProperty(s, "0", { value: "x" }), "Reflect.define rejects");
Object.defineProperty(s, "0", { value: "a" });
Object.defineProperty(s, "0", {});
Object.defineProperty(s, "0", { writable: false, enumerable: true, configurable: false });
throws(() => Object.defineProperty(s, "0", { writable: true }), "cannot become writable");
throws(() => Object.defineProperty(s, "0", { enumerable: false }), "cannot hide");
throws(() => Object.defineProperty(s, "0", { configurable: true }), "cannot become configurable");
throws(() => Object.defineProperty(s, "0", { get() { return "a"; } }), "cannot become accessor");
throws(() => { "use strict"; s[0] = "x"; }, "strict assignment");
throws(() => { "use strict"; delete s[0]; }, "strict delete");
check(s[0] === "a", "rejected operations preserve index");
check(delete s[7] && !Object.hasOwn(s, "7"), "delete expando");
Object.preventExtensions(s);
Object.defineProperty(s, "0", { value: "a" });
check(!Reflect.defineProperty(s, "8", { value: "new" }), "no new property after preventExtensions");
for (const lock of [Object.seal, Object.freeze]) {
  const locked: any = lock(Object("ab"));
  check(Object.isSealed(locked) && Object.isFrozen(locked), "immutable sealed indices");
  equal(Object.keys(locked), ["0", "1"], "locked enumeration");
  check(!Reflect.deleteProperty(locked, "1"), "locked delete");
}

// Own virtual indices must shadow inherited numeric accessors/properties.
const proto: any = { 0: "wrong", inherited: 1 };
const changed: any = Object("ab");
Object.setPrototypeOf(changed, proto);
check(changed[0] === "a" && changed.inherited === 1, "custom prototype");
Object.defineProperty(proto, "1", { get() { return "wrong"; } });
check(changed[1] === "b", "own index shadows inherited getter");
Object.setPrototypeOf(changed, null);
check(changed[0] === "a" && Object.hasOwn(changed, "1"), "null prototype");

// Wide expando objects use a separate ownership index; its miss is not proof
// that a virtual character property is absent.
const wide: any = Object("abc");
for (let i = 0; i < 80; i++) wide["field" + i] = i;
check(Object.hasOwn(wide, "1"), "wide own index");
Object.defineProperty(wide, "1", { value: "b" });
throws(() => Object.defineProperty(wide, "1", { value: "x" }), "wide incompatible definition");

const changing: any = Object("ab");
Object.defineProperty(changing, "first", { enumerable: true, get() {
  delete changing.later;
  Object.defineProperty(changing, "hiddenLater", { enumerable: false });
  return 5;
} });
changing.later = 6;
changing.hiddenLater = 7;
equal(Object.values(changing), ["a", "b", 5], "getter changes later keys");

function capture() { return Object(this); }
const methods: any = { capture };
const a: any = methods.capture.call("x".repeat(200));
const b: any = methods.capture.apply("x".repeat(200), []);
check(typeof a === "object" && a !== b && a.length === 200 && b[199] === "x", "call/apply wrappers");
a.extra = 4;
check(b.extra === undefined, "independent receiver state");
function strictReceiver() { "use strict"; return typeof this; }
check(strictReceiver.call("abc") === "string", "strict primitive receiver");
(String.prototype as any).issue9810 = capture;
const methodThis: any = ("abc" as any).issue9810();
check(typeof methodThis === "object" && methodThis[2] === "c", "prototype method receiver");
delete (String.prototype as any).issue9810;
equal(Object.keys(Object("")), [], "empty wrapper");
console.log("virtual-string-indices-9810 ok");
