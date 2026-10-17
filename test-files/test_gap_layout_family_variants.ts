// Object model case 2c, creation split: a family whose constructor stores a
// parameter (`kind`) is created through factories that pass a constant for
// it; each constant's births get a layout with the keys their creation sites
// store. Every observation must match node: own keys in insertion order
// whatever order the late keys are filled in (in, out of and across the
// layouts' key orders), absent keys through the prototype chain, deletes,
// re-adds, freezing, and stores through warmed store sites.
"use strict";
import * as util from "node:util";

function show(label: string, o: any) {
  const forIn: string[] = [];
  for (const k in o) forIn.push(k);
  console.log(
    label,
    Object.keys(o).join(","),
    "|", forIn.join(","),
    "|", JSON.stringify(o),
    "|", JSON.stringify({ ...o }),
    "|", Reflect.ownKeys(o).map(String).join(","),
    "|", Object.values(o).map(String).join(","),
  );
  console.log(label, util.inspect(o));
}

class Node {
  constructor(kind: number, pos: number, end: number) {
    (this as any).pos = pos;
    (this as any).end = end;
    (this as any).kind = kind;
    (this as any).flags = 0;
    (this as any).parent = undefined;
  }
  isKind(k: number) {
    return (this as any).kind === k;
  }
}
(Node.prototype as any).inherited = "proto";

function createBase(kind: number): any {
  return new Node(kind, -1, -1);
}
function createDecl(kind: number): any {
  const node = createBase(kind);
  node.symbol = undefined;
  node.localSymbol = undefined;
  return node;
}
function createBinary(left: any, op: number, right: any): any {
  const node = createBase(10);
  node.left = left;
  node.operator = op;
  node.right = right;
  node.flags |= 1;
  return node;
}
function createProperty(name: string, init: any): any {
  const node = createDecl(20);
  node.name = name;
  node.initializer = init;
  node.questionToken = undefined;
  return node;
}
function createCall(expr: any, args: any[]): any {
  const node = createBase(30);
  node.expression = expr;
  node.typeArguments = undefined;
  node.arguments = args;
  return node;
}
function createOther(kind: number): any {
  // A kind no factory passes as a constant: the family's own layout.
  const node = createBase(kind);
  node.text = "t" + kind;
  return node;
}

// Later phases: keys stored on any node, in varying orders.
function bind(node: any, sym: any) {
  if (node.kind !== 0 && node.flags >= 0) {
    node.symbol = sym;
    node.locals = sym ? [sym] : undefined;
  }
}
function flow(node: any, f: any) {
  if (node.kind !== 0 && node.pos !== 1) node.flowNode = f;
}
function attachDoc(node: any, d: any) {
  if (node.end !== 1 && node.kind >= 0) node.jsDoc = d;
}

const a = createBinary(1, 2, 3);
show("binary", a);
bind(a, "s1");
flow(a, "f1");
show("binary+bind+flow", a);

// Phase keys first, then a creation key that was never filled (out of the
// layout's key order).
const b = createBase(10);
flow(b, "f2");
attachDoc(b, ["doc"]);
b.left = "late-left";
bind(b, "s2");
b.right = "late-right";
show("binary out of order", b);
console.log("absent", b.operator, "operator" in b, b.inherited, "inherited" in b, Object.hasOwn(b, "inherited"));

const p = createProperty("x", 5);
show("property", p);
attachDoc(p, ["d"]);
flow(p, "fp");
bind(p, "sp");
show("property+phases", p);
delete p.name;
show("property-deleted", p);
p.name = "again";
show("property-readded", p);

const c = createCall("callee", [1, 2]);
flow(c, "fc");
c.extra = "dyn";
(c as any)["comp" + "uted"] = 7;
show("call+dynamic", c);

const o = createOther(99);
bind(o, "so");
o.left = "not-in-this-layout";
show("other kind", o);

// A key filled before the constructor-key-order stores of another phase.
const d = createProperty("y", 6);
d.questionToken = "?";
d.symbol = "sym-d";
d.initializer = 8;
show("overwrite", d);

// Warmed store sites, then a frozen and a sealed receiver.
function setLeft(n: any, v: any) {
  n.left = v;
}
function setRight(n: any, v: any) {
  n.right = v;
}
for (let i = 0; i < 50; i++) {
  const n = createBinary(i, 0, i);
  setLeft(n, i + 1);
  setRight(n, i + 2);
}
const fz = createBinary(1, 1, 1);
Object.freeze(fz);
try {
  setLeft(fz, 99);
  console.log("frozen store did not throw", fz.left);
} catch (e: any) {
  console.log("frozen store", e.constructor.name, fz.left);
}
const sealed = createBase(10);
Object.seal(sealed);
try {
  setRight(sealed, 5);
  console.log("sealed add did not throw", sealed.right);
} catch (e: any) {
  console.log("sealed add", e.constructor.name, "right" in sealed);
}
const hole = createBase(10);
setLeft(hole, "first");
setRight(hole, "second");
show("warmed site fills", hole);

// A setter on the prototype for a late key name, defined after births.
const before = createBase(30);
Object.defineProperty(Node.prototype, "arguments", {
  set(v) {
    console.log("setter ran", Array.isArray(v));
  },
  get() {
    return "getter";
  },
  configurable: true,
});
const after = createCall("e", [3]);
before.arguments = [4];
show("setter after", after);
show("setter before", before);

let sum = 0;
for (let i = 0; i < 200; i++) {
  const n = i % 3 === 0 ? createBinary(i, 1, i) : i % 3 === 1 ? createProperty("p" + i, i) : createCall(i, []);
  if (i % 2) bind(n, i);
  flow(n, i);
  sum += Object.keys(n).length;
}
console.log("sum", sum);
