// Enum objects written as the IIFEs tsc and esbuild emit. A fresh enum object
// is born from one constant image; every case the image cannot prove (the
// object already exists, a value is not constant, a store has side effects)
// must keep the ordinary keyed stores. Key order, the reverse mapping in
// elements, aliases that overwrite a reverse entry, non-index numeric keys
// and later mutation must all match node.

function show(label: string, e: any) {
  console.log(label, JSON.stringify(e), Object.keys(e).join(","));
  console.log(label, Object.getOwnPropertyNames(e).length, e[0], e["1"], e[-1], e.length);
  const seen: string[] = [];
  for (const k in e) seen.push(k);
  console.log(label, seen.join("|"), Object.getPrototypeOf(e) === Object.prototype);
}

// esbuild form (typescript.js): numeric members, a negative, an alias that
// overwrites the reverse entry of 1, a fraction, a large value.
var Kind = /* @__PURE__ */ ((Kind5: any) => {
  Kind5[Kind5["Unknown"] = 0] = "Unknown";
  Kind5[Kind5["First"] = 1] = "First";
  Kind5[Kind5["Second"] = 2] = "Second";
  Kind5[Kind5["Alias"] = 1] = "Alias";
  Kind5[Kind5["LessThan"] = -1] = "LessThan";
  Kind5[Kind5["Half"] = 0.5] = "Half";
  Kind5[Kind5["Big"] = 4294967296] = "Big";
  Kind5[Kind5["Max"] = 4294967294] = "Max";
  Kind5[Kind5["Ten"] = 10] = "Ten";
  return Kind5;
})(Kind || {});
show("kind", Kind);
console.log(Kind.Alias, Kind[1], Kind.Big, Kind["4294967296"], Kind[4294967294], Kind[0.5]);

// String enum (no reverse mapping) and a mixed enum.
var Color = /* @__PURE__ */ ((Color2: any) => {
  Color2["Red"] = "red";
  Color2["Green"] = "green";
  Color2["Blue"] = "blue";
  return Color2;
})(Color || {});
show("color", Color);
var Mixed = /* @__PURE__ */ ((Mixed3: any) => {
  Mixed3[Mixed3["A"] = 3] = "A";
  Mixed3["B"] = "b";
  Mixed3[Mixed3["C"] = 1] = "C";
  Mixed3["3"] = "three";
  return Mixed3;
})(Mixed || {});
show("mixed", Mixed);

// tsc form: `var E; (function (E) { ... })(E || (E = {}));`
var Flags: any;
(function (Flags: any) {
  Flags[Flags["None"] = 0] = "None";
  Flags[Flags["A"] = 1] = "A";
  Flags[Flags["B"] = 2] = "B";
  Flags[Flags["AB"] = 3] = "AB";
})(Flags || (Flags = {}));
show("flags", Flags);

// Merged declarations: the second IIFE extends the object the first made.
var Merged = /* @__PURE__ */ ((Merged2: any) => {
  Merged2[Merged2["X"] = 0] = "X";
  return Merged2;
})(Merged || {});
var Merged = /* @__PURE__ */ ((Merged2: any) => {
  Merged2[Merged2["Y"] = 1] = "Y";
  return Merged2;
})(Merged || {});
show("merged", Merged);

// Referenced before its IIFE runs: hoisted `var` reads undefined first.
function peek() { return typeof Later === "undefined" ? "undef" : Later.Z; }
console.log("before", peek());
var Later = /* @__PURE__ */ ((Later2: any) => {
  Later2[Later2["Z"] = 7] = "Z";
  return Later2;
})(Later || {});
console.log("after", peek(), Later[7]);

// Pre-assigned before the IIFE: the existing object is extended in place.
var Pre: any = { keep: true };
var Pre = /* @__PURE__ */ ((Pre2: any) => {
  Pre2[Pre2["P"] = 0] = "P";
  return Pre2;
})(Pre || {});
show("pre", Pre);

// Non-constant values and side effects keep the ordinary stores.
let counter = 0;
const next = () => ++counter;
var Computed = /* @__PURE__ */ ((Computed2: any) => {
  Computed2[Computed2["A"] = next()] = "A";
  Computed2[Computed2["B"] = 5] = "B";
  Computed2[Computed2["C"] = Computed2.B + next()] = "C";
  return Computed2;
})(Computed || {});
show("computed", Computed);

// In a loop: the second iteration extends the first iteration's object.
const made: any[] = [];
for (let i = 0; i < 2; i++) {
  var Looped = /* @__PURE__ */ ((Looped2: any) => {
    Looped2[Looped2["L" + i] = i] = "L" + i;
    Looped2[Looped2["Fixed"] = 9] = "Fixed";
    return Looped2;
  })(Looped || {});
  made.push(Looped);
}
console.log("loop", made[0] === made[1], JSON.stringify(Looped));

// In a function body: each call bears a fresh object.
function factory() {
  var Local = /* @__PURE__ */ ((Local2: any) => {
    Local2[Local2["Q"] = 1] = "Q";
    Local2[Local2["R"] = 2] = "R";
    return Local2;
  })(Local || {});
  return Local;
}
const f1 = factory(), f2 = factory();
f1.Q = 100;
console.log("factory", f1 === f2, f1.Q, f2.Q, f2[1], JSON.stringify(f2));

// Every born object is an ordinary extensible object.
Kind.Extra = 42;
Kind[99] = "Extra";
delete Kind.Second;
show("mutated", Kind);
Object.freeze(Color);
console.log("frozen", Object.isFrozen(Color), Object.isFrozen(Flags));
console.log("own", Object.prototype.hasOwnProperty.call(Kind, "0"), Object.prototype.hasOwnProperty.call(Kind, 0), "Unknown" in Kind);
