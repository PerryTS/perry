// A `var` is function-scoped and hoisted: a declaration that appears textually
// AFTER a statement that reads/writes it is still bound (as `undefined`) from
// function entry. Perry honored this for function *declarations* (and arrows)
// but NOT for function *expressions*: the fn-expr lowering pre-registered the
// hoisted local but never emitted the undefined-initialised entry slot, so a
// read/write compiled BEFORE the late `var x = …` fell to a folded `undefined`.
//
// React 19's `cloneElement`/`createElement` are exactly this shape — one
// hoisted `propName` used by a `for (propName in config)` loop, then redeclared
// as `var propName = arguments.length - 2`. The loop body's
// `hasOwnProperty.call(config, propName)` saw `undefined`, so it dropped every
// prop and `<Button asChild>` (Radix Slot → cloneElement) rendered a bare `<a>`.

const hop = Object.prototype.hasOwnProperty;

// The minimal trigger: a function EXPRESSION with a `for-in` loop variable that
// is a `var` declared after the loop.
const clone = function (element: any, config: any, children?: any): any {
  const props: any = Object.assign({}, element.props);
  let key = element.key;
  if (null != config)
    for (propName in (void 0 !== config.key && (key = "" + config.key), config))
      !hop.call(config, propName) ||
        "key" === propName ||
        "__self" === propName ||
        "__source" === propName ||
        (props[propName] = config[propName]);
  var propName = arguments.length - 2; // hoisted; declared AFTER the loop, reused as a number
  if (1 === propName) props.children = children;
  return props;
};

const el = { props: { href: "/signup", children: "Get Started" }, key: null };
const config = { "data-slot": "button", "data-variant": "default", className: "px-6", href: "/signup", children: "Get Started" };
const out = clone(el, config);
console.log("clone keys:", Object.keys(out).join(","));
console.log("clone data-slot:", out["data-slot"]);
console.log("clone className:", out.className);

// A plain forward read of a hoisted `var` in a function expression: reads
// before the declaration are `undefined`, after are the assigned value.
const fwd = function (): string {
  const before = String(later);
  var later = 42;
  const after = String(later);
  return before + "/" + after;
};
console.log("forward var:", fwd());

// Arrow expressions must keep behaving identically (they already worked).
const arrowFwd = (): string => {
  const before = String(v);
  var v = 7;
  return before + "/" + String(v);
};
console.log("arrow var:", arrowFwd());

// A method (object-literal function expression) with the same hoist shape.
const obj = {
  run(o: any): string {
    const seen: string[] = [];
    for (k in o) seen.push(k + "=" + o[k]);
    var k: string;
    return seen.join(",");
  },
};
console.log("method for-in:", obj.run({ a: 1, b: 2, c: 3 }));

// Nested `var` inside an if-branch of a function expression, used by a sibling
// branch before its textual position (already worked; guards against regression).
const branchy = function (cond: boolean): number {
  if (cond) {
    var acc = 10;
  } else {
    acc = 20;
  }
  return acc;
};
console.log("branchy true:", branchy(true), "false:", branchy(false));
