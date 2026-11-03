// #3144's `.call`/`.apply` fold for builtin-prototype method aliases
// (`var e = Function.prototype.toString; e.call(x)` -> `x.toString()`) was keyed
// by BARE NAME, module-wide. Minified bundles reuse one parameter name in every
// function, so a callback PARAMETER `e` elsewhere had `e.call(this, a, b, c)`
// rewritten to `this.toString(a, b, c)`: with an undefined `this` that threw
// "Cannot read properties of undefined (reading 'toString')" (the
// unist-util-visit-parents `convert()` wrapper, reached from remark-gfm).

function tagOf(x: any): string {
  var e = Object.prototype.toString;
  return e.call(x);
}

function sourceOf(f: any): string {
  var e = Function.prototype.toString;
  return typeof e.call(f);
}

function brandOf(x: any): string {
  var e = Function.prototype.toString;
  try { return "no throw: " + e.call(x); } catch (err: any) { return err.name; }
}

function tail(x: any): string {
  var e = [].slice;
  return e.call(x, 1).join(",");
}

function castFactory(e: any) {
  return function (this: any, node: any, index: any, parent: any): boolean {
    return !!(node && e.call(this, node, typeof index == "number" ? index : void 0, parent || void 0));
  };
}

const isText = castFactory(function (this: any, node: any, index: any, parent: any) {
  return node.type === "text" && this === undefined && index === 2 && parent === undefined;
});

function sliceAlias(e: any, x: any): any {
  return e.call(x, 1);
}

function fold(): string {
  const m = [].slice;
  return m.call([1, 2, 3], 1).join(",");
}

console.log(tagOf([1, 2]));
console.log(tagOf(null));
console.log(sourceOf(tagOf));
console.log(brandOf({}));
console.log(tail([1, 2, 3]));
console.log(isText({ type: "text" }, 2, null));
console.log(isText({ type: "link" }, 2, null));
console.log(sliceAlias(function (this: any, k: number) { return this.n + k; }, { n: 41 }));
console.log(fold());
