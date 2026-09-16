// Array BINDING patterns and IteratorClose: which completions close the
// iterator. Compared byte-for-byte against node.

function makeIterable(label: string, values: unknown[], opts: { throwAt?: number; badValueAt?: number; badDoneAt?: number } = {}) {
  let i = 0;
  return {
    [Symbol.iterator]() {
      console.log(label, "open");
      return {
        next() {
          const index = i++;
          if (opts.throwAt === index) {
            throw new Error(label + " next threw at " + index);
          }
          if (opts.badValueAt === index) {
            return {
              done: false,
              get value() {
                throw new Error(label + " value getter threw at " + index);
              },
            };
          }
          if (opts.badDoneAt === index) {
            return {
              get done(): boolean {
                throw new Error(label + " done getter threw at " + index);
              },
              value: 0,
            };
          }
          if (index < values.length) {
            return { done: false, value: values[index] };
          }
          return { done: true, value: undefined };
        },
        return() {
          console.log(label, "return called");
          return { done: true };
        },
      };
    },
  };
}

function run(name: string, fn: () => unknown) {
  try {
    console.log(name, "->", JSON.stringify(fn()));
  } catch (e) {
    console.log(name, "threw:", (e as Error).message);
  }
}

// Not exhausted on normal completion: return() must be called.
run("short", () => {
  const [a, b] = makeIterable("short", [1, 2, 3]);
  return [a, b];
});
// Exhausted: no return().
run("exact", () => {
  const [a, b, c] = makeIterable("exact", [1, 2]);
  return [a, b, c];
});
// Holes still advance.
run("holes", () => {
  const [, b, , d] = makeIterable("holes", [1, 2, 3, 4, 5]);
  return [b, d];
});
// Rest drains.
run("rest", () => {
  const [a, ...rest] = makeIterable("rest", [1, 2, 3]);
  return [a, rest];
});
// next() throwing marks the iterator done: no return().
run("next-throws", () => {
  const [a, b] = makeIterable("next-throws", [1, 2, 3], { throwAt: 1 });
  return [a, b];
});
// The result's value getter throwing: no return().
run("value-getter-throws", () => {
  const [a, b] = makeIterable("value-getter", [1, 2, 3], { badValueAt: 1 });
  return [a, b];
});
// The result's done getter throwing: no return().
run("done-getter-throws", () => {
  const [a, b] = makeIterable("done-getter", [1, 2, 3], { badDoneAt: 0 });
  return [a, b];
});
// A throwing default initializer DOES close the iterator.
run("default-throws", () => {
  const [a, b = (() => { throw new Error("default threw"); })()] = makeIterable("default", [1, undefined, 3]);
  return [a, b];
});
// A nested pattern that throws closes the outer iterator.
run("nested-throws", () => {
  const throwing = {
    get x(): number {
      throw new Error("nested getter threw");
    },
  };
  const [a, { x }] = makeIterable("nested", [1, throwing, 3]) as any;
  return [a, x];
});
// Generators, strings, sparse arrays and parameters.
function* gen() {
  try {
    yield 1;
    yield 2;
    yield 3;
  } finally {
    console.log("generator finally");
  }
}
run("generator", () => {
  const [a, b] = gen();
  return [a, b];
});
run("string", () => {
  const [a, b, ...rest] = "héllo";
  return [a, b, rest];
});
run("sparse", () => {
  const sparse = [1, , 3];
  const [a, b, c, d] = sparse;
  return [a, b === undefined, c, d === undefined];
});
function params([a, b]: Iterable<unknown>, [c, , e]: unknown[]) {
  return [a, b, c, e];
}
run("params", () => params(makeIterable("param", [7, 8, 9]), [1, 2, 3, 4]));
// In a loop with closures capturing each iteration's bindings.
run("loop-closures", () => {
  const fns: (() => string)[] = [];
  for (let i = 0; i < 3; i++) {
    const [p, q] = [i, i * 10];
    fns.push(() => p + ":" + q);
  }
  return fns.map((f) => f());
});
