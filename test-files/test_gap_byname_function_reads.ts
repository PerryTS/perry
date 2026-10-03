// Property reads on function objects answered from the site's holder entry
// (#10497): a plain function's base shape pins its own keys and its
// [[Prototype]]; the rest of the chain is %Function.prototype% and
// %Object.prototype%. Each phase reads through the same sites long enough for
// them to cache, then changes what the shapes prove.

const N = 40;

function phase(label: string, f: (o: any) => string, objs: any[]) {
  let last = "";
  let changes = 0;
  for (let i = 0; i < N; i++) {
    const r = f(objs[i % objs.length]);
    if (r !== last) {
      changes++;
      last = r;
    }
  }
  console.log(label, last, changes);
}

function plainFn() {
  return 1;
}
const arrow = () => 2;
const fns: any[] = [plainFn, function other() { return 2; }, arrow];
const read = (f: any) => `${f.isBuffer}|${f.custom}|${typeof f.call}|${f.length}|${f.name.length > 0}`;

phase("fn.start", read, fns);
(Function.prototype as any).custom = "onFunctionProto";
phase("fn.fproto", read, fns);
delete (Function.prototype as any).custom;
phase("fn.fproto.gone", read, fns);
(Object.prototype as any).isBuffer = "onObjectProto";
phase("fn.oproto", read, fns);
delete (Object.prototype as any).isBuffer;
phase("fn.oproto.gone", read, fns);
(plainFn as any).isBuffer = "own"; // the function leaves its base shape
phase("fn.own", read, fns);
Object.setPrototypeOf(arrow, { custom: "setproto" });
phase("fn.setproto", (f) => `${f.custom}|${f.isBuffer}`, [arrow]);
Object.defineProperty(Function.prototype, "custom", { get() { return "getter"; }, configurable: true });
phase("fn.accessor", (f) => `${f.custom}`, [function late() {}]);
delete (Function.prototype as any).custom;

// async functions and generators inherit from other intrinsic prototypes.
async function af() {}
function* gf() {}
phase("fn.kinds", (f) => `${f.isBuffer}|${typeof f.constructor}`, [af, gf, af]);

// class constructors: statics, the parent chain, Function.prototype.
class Base {
  static s = "static";
}
class Derived extends Base {}
phase("fn.class", (c) => `${c.s}|${c.isBuffer}|${c.name}`, [Base, Derived]);

// bound functions.
const bound: any = plainFn.bind(null);
phase("fn.bound", (f) => `${f.isBuffer}|${f.name}`, [bound]);

// `constructor.isBuffer` on plain objects and buffers (axios' isBuffer).
function isBuffer(val: any) {
  return (
    val !== null &&
    val !== undefined &&
    val.constructor !== null &&
    val.constructor !== undefined &&
    typeof val.constructor.isBuffer === "function" &&
    val.constructor.isBuffer(val)
  );
}
const vals: any[] = [{ a: 1 }, { url: "/x" }, Buffer.from("ab"), { headers: {} }, [1, 2], "str", 5];
phase("isBuffer", (v) => `${isBuffer(v)}`, vals);
phase("isBuffer.each", (v) => `${typeof v.constructor.isBuffer}`, [vals[0], vals[2], vals[4]]);
(Object as any).isBuffer = () => true;
phase("isBuffer.patched", (v) => `${isBuffer(v)}`, [vals[0], vals[1]]);
delete (Object as any).isBuffer;
phase("isBuffer.restored", (v) => `${isBuffer(v)}`, [vals[0], vals[1]]);
