// Class expressions must capture live bindings assigned after the class.
const wrapVar = function () {
  var C = class _C { constructor(x: string) { (this as any).v = helper(x); } };
  var out = { K: C };
  var helper = (s: string) => "var:" + s;
  return out;
};
console.log((new (wrapVar().K)("a") as any).v);

const wrapLet = function () {
  let C = class { constructor(x: string) { (this as any).v = helperL(x); } };
  var box = { K: C };
  var helperL = (s: string) => "let:" + s;
  return box;
};
const wrapConst = function () {
  const C = class { constructor(x: string) { (this as any).v = helperC(x); } };
  var box = { K: C };
  var helperC = (s: string) => "const:" + s;
  return box;
};
console.log((new (wrapLet().K)("b") as any).v);
console.log((new (wrapConst().K)("c") as any).v);

const registry: any = {};
const register = (name: string, cls: any) => { registry[name] = cls; };
const wrapArg = function () {
  register("K", class { constructor(x: string) { (this as any).v = helperA(x); } });
  var helperA = (s: string) => "arg:" + s;
};
wrapArg();
console.log(new registry.K("d").v);

const __commonJS = (cb: any, mod?: any) => function __require() {
  return mod || (0, cb[Object.keys(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
};
const require_parse_options = __commonJS({
  "parse-options.js"(_exports: any, module: any) {
    const emptyOpts = Object.freeze({});
    module.exports = (options: any) => options && typeof options === "object" ? options : emptyOpts;
  },
});
const require_comparator = __commonJS({
  "comparator.js"(_exports: any, module: any) {
    const ANY = Symbol("SemVer ANY");
    const Comparator = class _Comparator {
      static get ANY() { return ANY; }
      loose: boolean;
      value: string;
      constructor(comp: any, options?: any) {
        options = parseOptions(options);
        if (comp instanceof _Comparator) comp = comp.value;
        this.loose = !!options.loose;
        this.value = String(comp);
      }
      toString() { return this.value; }
    };
    module.exports = Comparator;
    const parseOptions = require_parse_options();
  },
});
const Comparator = require_comparator();
const minimum = [new Comparator(">=0.0.0-0")];
const c2 = new Comparator(">=1.2.3", { loose: true });
console.log(String(minimum[0]), c2.value, c2.loose);

const wrapReassign = function () {
  var C = class { constructor() { (this as any).v = tag(); } };
  var out = { K: C };
  var tag = () => "first";
  tag = () => "second";
  return out;
};
console.log((new (wrapReassign().K)() as any).v);

const mk = function (t: string) {
  var prefix = "p" + t;
  var C = class { constructor() { (this as any).v = t + ":" + prefix; } };
  return C;
};
const A = mk("x");
const B = mk("y");
console.log((new A() as any).v, (new B() as any).v);
