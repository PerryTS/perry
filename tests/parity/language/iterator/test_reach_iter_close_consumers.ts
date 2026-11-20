// IteratorClose by a built-in consumer: a consumer that stops before its
// iterator is done calls the iterator's `return`. The whole-program solve
// made that call for a for-of and for destructuring only, and called the
// `return` body inactive while it ran (check mode `PERRY_VERIFY_FACTS=1`:
// "inactive body: body entered"; with `PERRY_FACTS_DCE=1` the removed body
// trapped).
let closes = 0;
let steps = 0;

// Yields entry pairs forever; `return` counts.
function pairs(): any {
  let n = 0;
  return {
    [Symbol.iterator]() {
      return this;
    },
    next() {
      steps++;
      return { value: ["k" + n, n++], done: false };
    },
    return() {
      closes++;
      return {};
    },
  };
}

// The iterator is a separate object from the iterable.
function separate(): any {
  return {
    [Symbol.iterator]() {
      let n = 0;
      return {
        next() {
          steps++;
          return { value: n++, done: false };
        },
        return() {
          closes += 10;
          return {};
        },
      };
    },
  };
}

// Three values, then done: a consumer that runs to the end never closes.
function finite(): any {
  let n = 0;
  return {
    [Symbol.iterator]() {
      return this;
    },
    next() {
      steps++;
      return n < 3 ? { value: ["k" + n, n++], done: false } : { value: undefined, done: true };
    },
    return() {
      closes += 100;
      return {};
    },
  };
}

function run(label: string, f: () => unknown): void {
  closes = 0;
  steps = 0;
  let out: string;
  try {
    out = "ok " + String(f());
  } catch (e) {
    out = "threw " + (typeof e === "string" ? e : (e as Error).name);
  }
  console.log(label + ":", out, "closes=" + closes, "steps=" + steps);
}

run("from-mapper-throw", () => Array.from(pairs(), () => { throw "mapper"; }));
run("from-mapper-throw-separate", () => Array.from(separate(), () => { throw "mapper"; }));
run("from-mapper-throw-at-2", () => Array.from(pairs(), (p: any) => { if (p[1] === 2) throw "third"; return p[1]; }));
function Locked(this: any) {
  Object.preventExtensions(this);
}
run("from-define-throw", () => (Array.from as any).call(Locked, pairs()));
run("from-finite", () => Array.from(finite()).length);
run("from-finite-mapped", () => Array.from(finite(), (p: any) => p[1]).join("|"));
run("spread-finite", () => [...finite()].length);
run("for-of-break", () => { for (const p of pairs()) { if (p[1] === 1) break; } return "left"; });
run("destructure", () => { const [a, b] = pairs(); return a[0] + b[0]; });
run("map-finite", () => new Map(finite()).size);
run("set-finite", () => new Set(finite()).size);
run("from-entries-finite", () => Object.keys(Object.fromEntries(finite())).join("|"));

// A generator's `finally` runs when the consumer closes it.
let finals = 0;
function* gen(): Generator<number> {
  try {
    yield 1;
    yield 2;
    yield 3;
  } finally {
    finals++;
  }
}
run("generator-mapper-throw", () => Array.from(gen(), () => { throw "gmapper"; }));
console.log("generator-finally:", finals);
run("generator-mapped", () => Array.from(gen(), (x) => x * 2).join("|"));
console.log("generator-finally-after-exhaustion:", finals);
