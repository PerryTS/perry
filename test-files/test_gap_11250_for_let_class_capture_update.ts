// #11250: a class evaluated inside `for (let i …)` closes over that
// iteration's binding of `i`. The loop head's own writes to `i` (the update
// expression, in either `i++` or `i = i + 1` form) happen after the
// per-iteration copy, so they must not leak into the class from the previous
// iteration — the last class used to observe the post-increment value.

function inFunction(): string {
  const classes: Array<new () => { get(): number }> = [];
  for (let i = 0; i < 3; i++) {
    class C {
      get(): number {
        return i;
      }
    }
    classes.push(C);
  }
  return classes.map((C) => new C().get()).join(",");
}
console.log("function body, i++:", inFunction());

function inFunctionStatic(): string {
  const classes: Array<{ s(): number }> = [];
  for (let i = 0; i < 3; i++) {
    class C {
      static s(): number {
        return i;
      }
    }
    classes.push(C);
  }
  return classes.map((C) => C.s()).join(",");
}
console.log("function body, static:", inFunctionStatic());

const exprs: Array<new () => { get(): number }> = [];
for (let i = 0; i < 3; i = i + 1) {
  exprs.push(
    class {
      get(): number {
        return i;
      }
    },
  );
}
console.log("module, i = i + 1:", exprs.map((C) => new C().get()).join(","));

// An in-body write belongs to the current iteration and IS visible.
const bodyWrites: Array<new () => { get(): number }> = [];
for (let i = 0; i < 6; i++) {
  class C {
    get(): number {
      return i;
    }
  }
  i++;
  bodyWrites.push(C);
}
console.log("in-body write:", bodyWrites.map((C) => new C().get()).join(","));

// `var` has a single shared binding: every class sees the final value.
const shared: Array<new () => { get(): number }> = [];
for (var j = 0; j < 3; j++) {
  shared.push(
    class {
      get(): number {
        return j;
      }
    },
  );
}
console.log("var head:", shared.map((C) => new C().get()).join(","));
