// #10509: a function that only READS `arguments` — `arguments.length` and
// `arguments[k]` in value position — no longer builds an Arguments object per
// call. The reads run against the argument bundle the caller already passes,
// and every key that is not an own element (`"callee"`, symbols, inherited
// names, out-of-range and fractional numbers) falls back to the object the
// prologue would have built. Everything below must print exactly what Node
// prints, whichever path each read takes.
//
// This file is strict ESM (the repo package is `"type": "module"`); the
// sloppy-mode half — mapped parameters and a real `callee` — is
// `test_gap_10509_arguments_reads_sloppy.cts`.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

function show(label: string, read: () => unknown) {
  try {
    const v = read();
    const shown =
      typeof v === "function" ? "function:" + (v as Function).name : typeof v === "symbol" ? "symbol" : String(v);
    console.log(label + ": " + typeof v + " " + shown);
  } catch (e) {
    console.log(label + ": " + (e as Error).constructor.name);
  }
}

// (1) The Babel/validator default-parameter shape the issue measured.
function mergeArgs(): any {
  "use strict";
  const obj = arguments.length > 0 && arguments[0] !== undefined ? arguments[0] : { fresh: true };
  const defaults = arguments.length > 1 ? arguments[1] : undefined;
  return [JSON.stringify(obj), String(defaults), arguments.length].join(" ");
}
console.log("merge ()", mergeArgs());
console.log("merge (undefined)", (mergeArgs as any)(undefined));
console.log("merge ({a})", (mergeArgs as any)({ a: 1 }));
console.log("merge ({a}, 2)", (mergeArgs as any)({ a: 1 }, 2));
console.log("merge spread", (mergeArgs as any)(...[{ s: 1 }, "d", 3]));

// (2) Every key shape against a strict elided object.
// A user rest parameter makes the list non-simple, so the object is unmapped
// and its `callee` is the throwing accessor in any mode.
function strictKey(k: any, ...rest: any[]): any {
  return arguments[k];
}
function strictKeys() {
  const keys: any[] = [0, 1, 2, 3, -0, -1, 1.5, NaN, Infinity, "0", "1", "01", "length", "toString",
    "hasOwnProperty", "constructor", "map", "callee"];
  for (const k of keys) {
    show("strict[" + (typeof k === "string" ? JSON.stringify(k) : Object.is(k, -0) ? "-0" : String(k)) + "]",
      () => (strictKey as any)(k, "b", "c"));
  }
  show("strict[@@toStringTag]", () => (strictKey as any)(Symbol.toStringTag));
  show("strict[@@unscopables]", () => (strictKey as any)(Symbol.unscopables));
}
strictKeys();

function symbolMember(): any {
  "use strict";
  return (arguments as any)[Symbol.unscopables].at;
}
show("arguments[@@unscopables].at", () => symbolMember());

// (3) Numeric loops, including arithmetic context and the Babel rest shape.
function sumAll(): number {
  "use strict";
  let s = 0;
  for (let i = 0; i < arguments.length; i++) s += arguments[i] * 2;
  return s;
}
console.log("sumAll", (sumAll as any)(1, 2, 3, 4), (sumAll as any)(), (sumAll as any)(10));
function babelRest(first: any): any[] {
  "use strict";
  for (var _len = arguments.length, rest = new Array(_len > 1 ? _len - 1 : 0), _key = 1; _key < _len; _key++) {
    rest[_key - 1] = arguments[_key];
  }
  return [first, rest.length, rest.join(",")];
}
console.log("babelRest", JSON.stringify((babelRest as any)("a", "b", "c")), JSON.stringify((babelRest as any)("x")));
function lastArg(): any {
  "use strict";
  return arguments[arguments.length - 1];
}
console.log("lastArg", (lastArg as any)(1, 2, 3), (lastArg as any)());

// (4) An out-of-range index never sees Array.prototype: the bundle is an
// Array, the object it stands in for is not.
(Array.prototype as any)[8] = "array-proto";
function inherited(): any {
  "use strict";
  return [arguments[8], arguments[0]].join("|");
}
console.log("inherited", (inherited as any)("own"));
delete (Array.prototype as any)[8];

// (5) Uses that must keep the real object.
function asReceiver(): any {
  "use strict";
  return arguments[0]();
}
console.log("receiver", (asReceiver as any)(function (this: any) {
  return Object.prototype.toString.call(this) + " " + Array.isArray(this) + " " + this.length;
}));
function deleteIndex(): any {
  "use strict";
  const removed = delete arguments[0];
  return [removed, arguments.length, arguments[0], arguments[1]].join("|");
}
console.log("delete index", (deleteIndex as any)("a", "b"));
function deleteLength(): any {
  "use strict";
  const removed = delete (arguments as any).length;
  return [removed, (arguments as any).length].join("|");
}
console.log("delete length", (deleteLength as any)("a", "b"));
function captured(): any {
  "use strict";
  const read = () => arguments[1];
  return read();
}
console.log("captured", (captured as any)("a", "b"));
function defaulted(a = arguments[1]): any {
  return [a, arguments.length].join("|");
}
console.log("defaulted", (defaulted as any)(undefined, "from-args"), (defaulted as any)("own"));

// (6) Methods, closures and async bodies.
class K {
  m(): any {
    return [arguments.length, arguments[1], typeof arguments["callee" as any]].join("|");
  }
  static s(): any {
    return arguments[0];
  }
}
show("method", () => (new K() as any).m("a", "b"));
console.log("static", (K as any).s("st"));
const literal = {
  m(): any {
    return arguments[2];
  },
};
console.log("literal", (literal as any).m(1, 2, 3));
const expr = function (): any {
  "use strict";
  return arguments[0] + arguments.length;
};
console.log("fn expr", (expr as any)(40, 1));
async function asyncRead(): Promise<any> {
  return arguments[0];
}

// (7) Many cold reads: each one builds a throwaway object, so this churns the
// nursery while the key and bundle stay live across the fallback.
function coldReads(): number {
  "use strict";
  let n = 0;
  for (let i = 0; i < 20000; i++) {
    const k = i % 3 === 0 ? "toString" : i % 3 === 1 ? String(i % 2) : "length";
    const v = (strictKey as any)(k, "p" + i, { i });
    if (typeof v === "function") n += 1;
    else if (typeof v === "string") n += v.length;
    else n += v;
  }
  return n;
}
console.log("cold reads", coldReads());

asyncRead("async-value").then((v) => console.log("async", v));
