// #10490: a dynamic method call on a receiver whose prototype was replaced
// (`Object.setPrototypeOf(o, proto); o.run()`) restored a STALE caller `this`
// after an evacuating minor ran inside the callee.
//
// `js_native_call_method`'s early path for such receivers bound the callee's
// `this` with a private `ImplicitThisScope` guard that kept the DISPLACED
// value — the caller's receiver — in a plain struct field and wrote it back in
// `Drop`. A copying minor inside the call relocates that object and rewrites
// every slot the collector can see; a Rust field is not one, so the restore
// reinstalled a retired from-space address and the caller's next `this.tag`
// read `undefined` (`Cannot read properties of undefined (reading 'length')`,
// cheerio's `this.options` in `_findBySelector`). The #9445 sweep rooted every
// `let prev = js_implicit_this_set(..)`, but not the same save hidden in a
// guard's `Drop` — nor the two identical guards behind the `Array.prototype`
// callback methods (`DenseThisGuard`, `ThisGuard`).
//
// Every case builds a fresh (young) receiver whose `run` reads `this`
// dynamically, calls something that allocates past the nursery, then reads
// `this` again. Deterministic without GC env knobs: pre-fix the affected cases
// print a non-zero `bad=` count; node prints `bad=0` for every line.

const N = 500;

function churn(): number {
  const tmp: any[] = [];
  for (let k = 0; k < 2000; k++) tmp.push({ k: k, s: "t" + k, pad: [k, k + 1] });
  return tmp.length;
}

function check(name: string, make: (i: number) => any, invoke?: (o: any) => any): void {
  let bad = 0;
  const notes: string[] = [];
  for (let i = 0; i < N; i++) {
    const o: any = make(i);
    const want = "tag" + i + ":2000";
    let got: any;
    try {
      got = invoke ? invoke(o) : o.run();
    } catch (e: any) {
      got = "THREW:" + (e && e.message);
    }
    if (got !== want) {
      bad++;
      if (bad <= 2) notes.push(" [" + i + " got=" + String(got) + "]");
    }
  }
  console.log(name + " bad=" + bad + notes.join(""));
}

// Plain-function methods: `this` is read off the implicit-`this` cell.
function run(this: any): string {
  const n = this.churn(); // dynamic method call; a moving minor runs inside it
  return this.tag + ":" + n; // `this` must still be the (moved) receiver
}
function runViaCall(this: any): string {
  const n = this.churn.call(this);
  return this.tag + ":" + n;
}
function runViaApply(this: any): string {
  const n = this.churn.apply(this, []);
  return this.tag + ":" + n;
}
const proto: any = {
  churn: function (this: any): number {
    return churn();
  },
  run: run,
  runViaCall: runViaCall,
  runViaApply: runViaApply,
};

// --- receivers whose prototype was replaced -------------------------------

check(
  "setPrototypeOf_object",
  (i) => {
    const o = { tag: "tag" + i };
    Object.setPrototypeOf(o, proto);
    return o;
  },
);

check(
  "object_create",
  (i) => {
    const o = Object.create(proto);
    o.tag = "tag" + i;
    return o;
  },
);

check(
  "proto_literal",
  (i) => ({ __proto__: proto, tag: "tag" + i }),
);

class Box {
  tag: string;
  constructor(i: number) {
    this.tag = "tag" + i;
  }
  run(): string {
    return "box";
  }
}

check(
  "class_instance_swapped_to_object",
  (i) => {
    const b = new Box(i);
    Object.setPrototypeOf(b, proto);
    return b;
  },
);

class Other {
  tag = "";
  churn(): number {
    return churn();
  }
  run(): string {
    const n = this.churn();
    return this.tag + ":" + n;
  }
}

check(
  "class_instance_swapped_to_class",
  (i) => {
    const b: any = new Box(i);
    Object.setPrototypeOf(b, Other.prototype);
    return b;
  },
);

// --- the outer or inner call through Function.prototype.call / apply -------

check(
  "outer_call",
  (i) => {
    const o = { tag: "tag" + i };
    Object.setPrototypeOf(o, proto);
    return o;
  },
  (o) => o.run.call(o),
);

check(
  "outer_apply",
  (i) => {
    const o = { tag: "tag" + i };
    Object.setPrototypeOf(o, proto);
    return o;
  },
  (o) => o.run.apply(o, []),
);

check(
  "inner_call",
  (i) => {
    const o = { tag: "tag" + i };
    Object.setPrototypeOf(o, proto);
    return o;
  },
  (o) => o.runViaCall(),
);

check(
  "inner_apply",
  (i) => {
    const o = { tag: "tag" + i };
    Object.setPrototypeOf(o, proto);
    return o;
  },
  (o) => o.runViaApply(),
);

// --- a class evaluated per factory call, methods mixed into its base --------
// (cheerio's `load()` declares `class LoadedCheerio extends Cheerio` per call
// and installs its API with `Object.assign(Cheerio.prototype, ...)`.)

class Base {
  tag: string;
  constructor(i: number) {
    this.tag = "tag" + i;
  }
}
Object.assign(Base.prototype, { churn: proto.churn, run: run });

function makeLoaded(): any {
  class Loaded extends Base {}
  return Loaded;
}

check(
  "per_evaluation_subclass_mixin",
  (i) => {
    const Loaded = makeLoaded();
    return new Loaded(i);
  },
);

// --- Array.prototype callback methods, called from a `this`-reading method --
// The same unrooted save sat in the dense (`DenseThisGuard`) and array-like
// (`ThisGuard`) callback engines. The host method is deliberately NOT named
// like any class method above: a class-declared `run` routes `o.run()` through
// a dispatch that rebinds `this` into the callee's captures, which hides the
// stale cell from the body.

function host(i: number, body: (this: any) => string): any {
  return { tag: "tag" + i, hostRun: body };
}

function callHost(o: any): any {
  return o.hostRun();
}

check(
  "array_forEach",
  (i) =>
    host(i, function (this: any) {
      let n = 0;
      [1].forEach(function () {
        n = churn();
      });
      return this.tag + ":" + n;
    }),
  callHost,
);

check(
  "array_map",
  (i) =>
    host(i, function (this: any) {
      const out = [1].map(function () {
        return churn();
      });
      return this.tag + ":" + out[0];
    }),
  callHost,
);

check(
  "array_filter_some_every_find",
  (i) =>
    host(i, function (this: any) {
      let n = 0;
      [1].filter(function () {
        n = churn();
        return true;
      });
      [1].some(function () {
        n = churn();
        return false;
      });
      [1].every(function () {
        n = churn();
        return true;
      });
      [1].find(function () {
        n = churn();
        return false;
      });
      return this.tag + ":" + n;
    }),
  callHost,
);

check(
  "array_forEach_thisArg",
  (i) =>
    host(i, function (this: any) {
      const arr: number[] = [1, 2].slice(1);
      let n = 0;
      const target = { k: 1 };
      arr.forEach(function (this: any) {
        n = churn() + (this === target ? 0 : 1000);
      }, target);
      return this.tag + ":" + n;
    }),
  callHost,
);

check(
  "arraylike_forEach_thisArg",
  (i) =>
    host(i, function (this: any) {
      let n = 0;
      const target = { k: 1 };
      Array.prototype.forEach.call(
        { length: 1, 0: 1 },
        function (this: any) {
          n = churn() + (this === target ? 0 : 1000);
        },
        target,
      );
      return this.tag + ":" + n;
    }),
  callHost,
);

check(
  "arraylike_map_thisArg",
  (i) =>
    host(i, function (this: any) {
      const target = { k: 1 };
      const out: any = Array.prototype.map.call(
        { length: 1, 0: 1 },
        function (this: any) {
          return churn() + (this === target ? 0 : 1000);
        },
        target,
      );
      return this.tag + ":" + out[0];
    }),
  callHost,
);

// --- the swapped receiver is itself the caller of a second swapped call ----

const outerProto: any = {
  run: function (this: any): string {
    const inner: any = { tag: "inner" };
    Object.setPrototypeOf(inner, proto);
    const r = inner.run(); // nested early-path call on another swapped receiver
    return this.tag + ":" + r.slice(r.indexOf(":") + 1);
  },
};

check(
  "nested_swapped_receivers",
  (i) => {
    const o = { tag: "tag" + i };
    Object.setPrototypeOf(o, outerProto);
    return o;
  },
);
