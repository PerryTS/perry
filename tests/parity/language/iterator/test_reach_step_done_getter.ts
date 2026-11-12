// IteratorComplete: every consumer of the iteration protocol reads `done`
// of a step result before its value, so a getter under `done` runs (and one
// under `value` runs only when the result is not done). The whole-program
// solve read the value only and called the `done` getter's body inactive
// while it ran (check mode `PERRY_VERIFY_FACTS=1`: "inactive body: body
// entered"; with `PERRY_FACTS_DCE=1` the removed body trapped).
let log: string[] = [];
function flush(label: string): void {
  console.log(label + ":", log.join(","));
  log = [];
}

// `n` values, then done; `done` and `value` are getters on every result.
function counted(n: number, tag: string): any {
  let i = 0;
  return {
    [Symbol.iterator]() {
      return this;
    },
    next() {
      const k = i++;
      return {
        get done() {
          log.push(tag + "-done" + k);
          return k >= n;
        },
        get value() {
          log.push(tag + "-value" + k);
          return k * 10;
        },
      };
    },
  };
}

// The iterator is what `[Symbol.iterator]()` returns, not the iterable.
function separate(n: number, tag: string): any {
  return {
    [Symbol.iterator]() {
      let i = 0;
      return {
        next() {
          const k = i++;
          return {
            get done() {
              log.push(tag + "-done" + k);
              return k >= n;
            },
            value: k,
          };
        },
      };
    },
  };
}

const [a, b] = counted(3, "d");
console.log("destructure:", a, b);
flush("destructure");

const [c, ...rest] = counted(3, "r");
console.log("rest:", c, rest.join("|"));
flush("rest");

const [only] = counted(0, "e");
console.log("empty:", only);
flush("empty");

let sum = 0;
for (const x of counted(2, "f")) sum += x;
console.log("for-of:", sum);
flush("for-of");

for (const x of counted(5, "b")) {
  if (x === 10) break;
}
flush("for-of-break");

console.log("spread:", [...counted(2, "s")].join("|"));
flush("spread");

function count(...xs: number[]): number {
  return xs.length;
}
console.log("call-spread:", count(...counted(2, "c")));
flush("call-spread");

console.log("array-from:", Array.from(counted(2, "a")).join("|"));
flush("array-from");

console.log("array-from-mapped:", Array.from(counted(2, "m"), (x: number) => x + 1).join("|"));
flush("array-from-mapped");

console.log("set:", new Set(counted(2, "t")).size);
flush("set");

console.log("max:", Math.max(...counted(2, "x")));
flush("max");

function* delegate(): any {
  yield* counted(2, "y");
}
console.log("yield*:", [...delegate()].join("|"));
flush("yield*");

let sep = 0;
for (const x of separate(2, "p")) sep += x;
console.log("separate:", sep, [...separate(1, "q")].length);
flush("separate");

// Controls: a plain `done` data property, an array, a generator.
function plain(n: number): any {
  let i = 0;
  return {
    [Symbol.iterator]() {
      return this;
    },
    next() {
      return { done: i >= n, value: i++ };
    },
  };
}
function* gen() {
  yield 1;
  yield 2;
}
const [p0, p1] = plain(2);
console.log("plain:", p0, p1, [...plain(3)].join("|"), [...gen()].join("|"), Array.from([7, 8]).join("|"));
