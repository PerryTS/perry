// #10424: the Function constructor applies ToString to every argument, left
// to right, and a spread argument list passes its elements. A non-string
// argument used to read as "" (an array of parameter names lost its
// parameters) and a spread array became a single argument (an empty body).
const body = ["return a", "+", "b"].join(" ");

function show(label: string, make: () => any): void {
  try {
    const f = make();
    console.log(label, typeof f, typeof f === "function" ? f(2, 3) : String(f));
  } catch (e) {
    console.log(label, "threw", (e as Error).name);
  }
}

show("string params (control)", () => new Function("a,b", body));
show("array params", () => new Function(["a", "b"] as any, body));
show("array params, constant body", () => new Function(["a", "b"] as any, "return a + b"));
show("call, array params", () => Function(["a", "b"] as any, "return a + b"));
show("toString params", () => new Function({ toString: () => "a,b" } as any, body));
show("numeric param", () => new Function(5 as any, body));
show("array body", () => new Function("a", "b", ["return a * b"] as any));
show("spread all", () => new Function(...["a", "b", body]));
show("spread tail", () => new Function("a", ...["b", body]));
show("spread head", () => new Function(...["a"], "b", body));
show("spread runtime list", () => {
  const names = "a,b".split(",");
  return new Function(...names, body);
});
show("array params length", () => {
  const f = new Function(["a", "b", "c"] as any, body);
  return f.length;
});

// Conversion is left to right and happens before the source is parsed.
const order: string[] = [];
function tracked(name: string, value: string): any {
  return {
    toString() {
      order.push(name);
      return value;
    },
  };
}
show("tracked", () => new Function(tracked("p1", "a"), tracked("p2", "b"), tracked("body", body)));
console.log("order", order.join(","));
order.length = 0;
show("tracked syntax error", () => new Function(tracked("p1", "a"), tracked("body", "return )")));
console.log("order before syntax error", order.join(","));

try {
  new Function(
    {
      toString() {
        throw new RangeError("from toString");
      },
    } as any,
    body,
  );
  console.log("toString throw: none");
} catch (e) {
  console.log("toString throw", (e as Error).name, (e as Error).message);
}
try {
  new Function(Symbol("s") as any, body);
  console.log("symbol: none");
} catch (e) {
  console.log("symbol", (e as Error).name);
}

// Rest parameters with a runtime body.
show("rest param", () => new Function("...xs", ["return xs.length * 10 +", "xs[1]"].join(" ")));
show("param then rest", () => new Function("a", "...rest", ["return a +", "rest.length"].join(" ")));

// lodash 4.18.1 `_.template` builds its render function this way
// (`lodash.js:14992`, `lodash/template.js:271`):
//   Function(importsKeys, sourceURL + 'return ' + source).apply(undefined, importsValues)
const importsKeys = ["_", "escape"];
const importsValues = [
  { upper: (s: string) => s.toUpperCase() },
  (s: string) => s.split("<").join("&lt;"),
];
const source = [
  "function(obj) {",
  "var __p = '';",
  "__p += 'Hello ' + escape(_.upper(obj.user)) + '!';",
  "return __p",
  "}",
].join("\n");
const sourceURL = "//# sourceURL=lodash.templateSources[0]\n";
const render = (Function as any)(importsKeys, sourceURL + "return " + source).apply(
  undefined,
  importsValues,
);
console.log("template", typeof render, render({ user: "<ann>" }));
