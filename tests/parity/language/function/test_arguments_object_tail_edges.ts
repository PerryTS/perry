// Arguments objects at the edges of their birth facts: mapped (sloppy, simple
// parameters) vs unmapped (defaults, rest) vs strict; `length` writes and
// deletes; extra named keys past the inline floor (spill and overflow);
// delete of indices, `length`, `callee` and named keys, and re-adds; mapped
// aliases after a delete; an arguments object escaping to a closure;
// freeze / preventExtensions; every reader after each mutation.

function show(label: string, a: any): void {
  const keys = Object.keys(a);
  const forin: string[] = [];
  for (const k in a) forin.push(k);
  const spread = { ...a };
  let arr: any[] = [];
  try { arr = Array.from(a); } catch (e) { arr = ["throw"]; }
  const applied = (function (...xs: any[]) { return xs.length + ":" + xs.join(","); }).apply(null, a);
  console.log(
    label,
    "len=" + String(a.length),
    "keys=" + keys.join(","),
    "forin=" + forin.join(","),
    "spread=" + Object.keys(spread).join(","),
    "json=" + JSON.stringify(a),
    "from=" + arr.join(","),
    "apply=" + applied,
  );
}

function mapped(x: any, y: any, z: any): any {
  show("mapped.birth", arguments);
  x = 10;
  console.log("mapped.alias", arguments[0], arguments[1]);
  (arguments as any)[1] = 21;
  console.log("mapped.back", y);
  delete (arguments as any)[0];
  x = 11;
  console.log("mapped.afterDelete", arguments[0], x, 0 in arguments);
  (arguments as any)[0] = 12;
  console.log("mapped.readd", arguments[0], x);
  show("mapped.end", arguments);
  return arguments;
}

function unmappedDefaults(x: any = 1, y: any = 2): any {
  x = 5;
  console.log("defaults.noalias", arguments[0], arguments.length);
  show("defaults", arguments);
  return arguments;
}

function unmappedRest(x: any, ...rest: any[]): any {
  x = 6;
  console.log("rest.noalias", arguments[0], rest.length, arguments.length);
  show("rest", arguments);
  return arguments;
}

function strict(x: any, y: any): any {
  "use strict";
  x = 7;
  console.log("strict.noalias", arguments[0]);
  let calleeThrows = false;
  try { void (arguments as any).callee; } catch (e) { calleeThrows = true; }
  console.log("strict.callee.throws", calleeThrows);
  show("strict", arguments);
  return arguments;
}

function lengthWrites(a: any, b: any, c: any): void {
  const args: any = arguments;
  args.length = 0;
  show("len0", args);
  args.length = 5;
  show("len5", args);
  args.length = "x";
  show("lenx", args);
  console.log("len.idx", args[0], args[1], args[2], args[3]);
  delete args.length;
  console.log("len.deleted", args.length, "length" in args, Object.prototype.hasOwnProperty.call(args, "length"));
  show("len.del", args);
  args.length = 2;
  show("len.readd", args);
}

function extraKeys(n: number, p: any, q: any): void {
  const args: any = arguments;
  for (let i = 0; i < n; i++) args["k" + i] = i * 3;
  console.log("extra" + n, args.length, args[0], args[1], args[2], args["k0"], args["k" + (n - 1)]);
  delete args["k0"];
  delete args[1];
  console.log("extra" + n + ".del", "k0" in args, 1 in args, args[1], args["k" + (n - 1)]);
  args["k0"] = "again";
  args[1] = "one";
  console.log("extra" + n + ".readd", args["k0"], args[1], p, args.length);
  show("extra" + n, args);
}

function deleteCallee(a: any): void {
  const args: any = arguments;
  console.log("callee.before", typeof args.callee, delete args.callee, "callee" in args);
  args.callee = "mine";
  console.log("callee.readd", args.callee);
  show("callee", args);
}

function escapes(x: any, y: any): () => string {
  const args: any = arguments;
  return function (): string {
    x = "X" + x;
    args.extra = "e";
    args[1] = "Y";
    for (let i = 0; i < 12; i++) args["late" + i] = i;
    return [args[0], x, y, args.extra, args.late11, args.length].join("|");
  };
}

function frozen(a: any, b: any): void {
  const args: any = arguments;
  args.named = 1;
  Object.freeze(args);
  let threw = false;
  try { (function () { "use strict"; args[0] = 9; })(); } catch (e) { threw = true; }
  console.log("frozen", Object.isFrozen(args), threw, args[0], a);
  show("frozen", args);
}

function noExtend(a: any): void {
  const args: any = arguments;
  Object.preventExtensions(args);
  args.more = 1;
  args[0] = "changed";
  console.log("noext", Object.isExtensible(args), args.more, args[0], a);
  show("noext", args);
}

mapped(1, 2, 3);
unmappedDefaults(8);
unmappedRest(1, 2, 3, 4);
strict(1, 2);
lengthWrites("a", "b", "c");
extraKeys(1, "p", "q");
extraKeys(8, "p", "q");
extraKeys(40, "p", "q");
deleteCallee(1);
const late = escapes("x", "y");
console.log("escape1", late());
console.log("escape2", late());
frozen(1, 2);
noExtend(1);
const kept = mapped(4, 5, 6);
show("kept", kept);
