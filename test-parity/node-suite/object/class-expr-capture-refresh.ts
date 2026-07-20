// #6604: a class EXPRESSION capturing an enclosing-function var that is
// assigned AFTER the class in source order (semver's ubiquitous
// `var Comparator = class _Comparator { … }; …; var parseOptions =
// require_parse_options()` CJS shape) must see the LIVE value when the class
// value escapes and is constructed DYNAMICALLY. The #6037/#6052 end-of-body
// capture-refresh machinery previously covered class DECLARATIONS only; the
// stale declaration-time snapshot made the captured var read `undefined`
// forever ("TypeError: value is not a function" at pi-native init).

// Shape 1: var-assigned NAMED class expression.
var wrapVar = function () {
  var C = class _C {
    constructor(x: string) {
      (this as any).v = helper(x);
    }
  };
  var out = { K: C };
  var helper = function (s: string) {
    return "var:" + s;
  };
  return out;
};
console.log("named var:", new (wrapVar().K)("a").v);

// Shape 2: let / const anonymous class expressions.
var wrapLet = function () {
  let C = class {
    constructor(x: string) {
      (this as any).v = helperL(x);
    }
  };
  var box = { K: C };
  var helperL = function (s: string) {
    return "let:" + s;
  };
  return box;
};
var wrapConst = function () {
  const C = class {
    constructor(x: string) {
      (this as any).v = helperC(x);
    }
  };
  var box = { K: C };
  var helperC = function (s: string) {
    return "const:" + s;
  };
  return box;
};
console.log("let:", new (wrapLet().K)("b").v);
console.log("const:", new (wrapConst().K)("c").v);

// Shape 3: class expression in ARGUMENT position (no binding statement).
var registry: any = {};
var register = function (name: string, cls: any) {
  registry[name] = cls;
};
var wrapArg = function () {
  register(
    "K",
    class {
      constructor(x: string) {
        (this as any).v = helperA(x);
      }
    }
  );
  var helperA = function (s: string) {
    return "arg:" + s;
  };
};
wrapArg();
console.log("arg:", new registry.K("d").v);

// Shape 4: the esbuild `__commonJS` semver comparator.js layout — class
// expression + `module.exports` + trailing requires, constructed dynamically
// from another wrapper at init time.
var __commonJS = (cb: any, mod: any = undefined) =>
  function __require() {
    return (
      mod || (0, cb[Object.keys(cb)[0]])((mod = { exports: {} }).exports, mod),
      mod.exports
    );
  };
var require_parse_options = __commonJS({
  "parse-options.js"(exports: any, module: any) {
    "use strict";
    var emptyOpts = Object.freeze({});
    module.exports = (options: any) =>
      options && typeof options === "object" ? options : emptyOpts;
  },
});
var require_comparator = __commonJS({
  "comparator.js"(exports: any, module: any) {
    "use strict";
    var ANY = Symbol("SemVer ANY");
    var Comparator = class _Comparator {
      static get ANY() {
        return ANY;
      }
      value: string;
      loose: boolean;
      constructor(comp: any, options?: any) {
        options = parseOptions(options);
        if (comp instanceof _Comparator) {
          comp = (comp as any).value;
        }
        this.loose = !!options.loose;
        this.value = String(comp);
      }
      toString() {
        return this.value;
      }
    };
    module.exports = Comparator;
    var parseOptions = require_parse_options();
  },
});
var Comparator = require_comparator();
var minimum = [new Comparator(">=0.0.0-0")];
var c2 = new Comparator(">=1.2.3", { loose: true });
console.log("semver:", String(minimum[0]), c2.value, c2.loose);

// Refresh must keep tracking REASSIGNMENTS of the captured var, not just its
// first initialization.
var wrapReassign = function () {
  var C = class {
    constructor() {
      (this as any).v = tag();
    }
  };
  var out = { K: C };
  var tag = function () {
    return "first";
  };
  tag = function () {
    return "second";
  };
  return out;
};
console.log("reassign:", new (wrapReassign().K)().v);

// Per-evaluation isolation of the NORMAL (assigned-before-class) shape must
// be preserved: two factory calls, two classes, distinct captured values.
var mk = function (t: string) {
  var prefix = "p" + t;
  var C = class {
    constructor() {
      (this as any).v = t + ":" + prefix;
    }
  };
  return C;
};
var A = mk("x");
var B = mk("y");
console.log("multi-eval:", new A().v, new B().v);
