// #11506: an Arguments object's exotic state (is-arguments, restricted
// `callee`, and the parameter aliases of a sloppy mapped object) lives on the
// object's own metadata record instead of a table keyed by its address. The
// record moves with the object, so everything below must behave identically
// whether or not the object was evacuated in between — the churn section
// keeps hundreds of them alive across many collections and then checks every
// alias in both directions.
//
// CommonJS, so functions are sloppy unless they say "use strict". No
// `require` on purpose: the module stays a plain script.
// Compared byte-for-byte against `node --experimental-strip-types`.

const isArguments = (value: unknown): boolean =>
  Object.prototype.toString.call(value) === "[object Arguments]";

function show(label: string, value: unknown): void {
  console.log(label + ":", value);
}

// Mapped aliasing runs both ways, for every passed parameter.
function mappedBothWays(a: any, b: any, c: any): string {
  a = "A";
  arguments[1] = "B";
  const out = [a, b, c, arguments[0], arguments[1], arguments[2]].join("|");
  return out;
}
show("mapped", (mappedBothWays as any)(1, 2, 3));

// CreateMappedArgumentsObject maps only the indices the call passed. A later
// write to such an index defines an ordinary property: it never reaches the
// parameter, and the parameter never reaches it.
function unpassed(a: any, b: any): string {
  arguments[1] = 5;
  const first = [b, arguments[1]].join("|");
  b = 7;
  return first + " " + [b, arguments[1], arguments.length].join("|");
}
show("unpassed", (unpassed as any)(1));

// `length` is an ordinary property: changing it leaves the aliases alone.
function lengthWrite(a: any, b: any): string {
  arguments.length = 0;
  a = "x";
  return [arguments.length, arguments[0], Array.prototype.slice.call(arguments).length].join("|");
}
show("length", (lengthWrite as any)(1, 2));

// `delete` breaks the alias for good, even once the index is redefined.
function deleteThenRedefine(a: any): string {
  delete arguments[0];
  arguments[0] = "again";
  a = "param";
  return [a, arguments[0]].join("|");
}
show("delete", (deleteThenRedefine as any)(1));

// defineProperty: a value writes through, writable:false and accessors unmap.
function defineWrites(a: any, b: any): string {
  Object.defineProperty(arguments, "0", { value: "via define" });
  const afterValue = a;
  Object.defineProperty(arguments, "0", { writable: false });
  a = "later";
  Object.defineProperty(arguments, "1", { get: () => "getter" });
  b = "b later";
  return [afterValue, a, arguments[0], arguments[1], b].join("|");
}
show("define", (defineWrites as any)(1, 2));

// Unmapped objects: strict code and a non-simple parameter list. Both carry
// the restricted `callee`.
function strictUnmapped(a: any): string {
  "use strict";
  arguments[0] = 7;
  const afterSet = a;
  a = 9;
  let callee = "no throw";
  try {
    void arguments.callee;
  } catch (e) {
    callee = (e as Error).constructor.name;
  }
  return [afterSet, arguments[0], callee].join("|");
}
show("strict", (strictUnmapped as any)(1));

function nonSimple(a: any = 0): string {
  arguments[0] = 7;
  const afterSet = a;
  let callee = "no throw";
  try {
    void arguments.callee;
  } catch (e) {
    callee = (e as Error).constructor.name;
  }
  return [afterSet, arguments[0], callee].join("|");
}
show("non-simple", (nonSimple as any)(1));

// A sloppy function with no parameters still has an ordinary `callee`.
function noParams(): string {
  return [arguments.length, arguments.callee === noParams].join("|");
}
show("no params", (noParams as any)(1, 2));

// Identity, as the probes see it.
const sloppyArgs = (function (x: any) {
  return arguments;
})(1);
const strictArgs = (function (x: any) {
  "use strict";
  return arguments;
})(1);
show("isArguments", [isArguments(sloppyArgs), isArguments(strictArgs), isArguments({ length: 0 })].join("|"));
show("spread", JSON.stringify([...sloppyArgs, ...strictArgs]));
show("from", JSON.stringify(Array.from(sloppyArgs)));

// Churn: keep many mapped objects alive across many collections, then check
// every alias in both directions.
function keep(a: any, b: any): any {
  return {
    args: arguments,
    set: (x: any) => {
      a = x;
    },
    get: () => a,
  };
}
const kept: any[] = [];
for (let i = 0; i < 500; i++) kept.push((keep as any)(i, "b" + i));
let junk: any[] = [];
for (let i = 0; i < 200000; i++) {
  junk.push({ i, s: "x" + i });
  if (junk.length > 1000) junk = [];
}
let ok = 0;
for (let i = 0; i < kept.length; i++) {
  const k = kept[i];
  k.set(i + 1);
  if (k.args[0] !== i + 1) break;
  k.args[0] = i + 2;
  if (k.get() !== i + 2) break;
  if (k.args[1] !== "b" + i) break;
  if (!isArguments(k.args)) break;
  ok++;
}
show("kept", ok + "/" + kept.length);
