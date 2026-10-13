// ehfree 2b: runtime catches are landing pads (perry_trap.ll), not setjmp.
// Each case sends a throw through a runtime trap (promise executor and
// reaction drains, generator resume, iterator factory, timer drain) with
// JS try/catch around and inside it; the output must match node.

function deep(n: number): number {
  if (n === 0) throw new Error("deep bottom");
  return deep(n - 1) + 1;
}

// 1. A throw out of a runtime callback at depth.
new Promise<number>(() => {
  deep(300);
}).catch((e) => console.log("1 executor at depth:", (e as Error).message));

// 2. Nested runtime -> JS -> runtime catches.
new Promise<void>((_resolve, reject) => {
  try {
    new Promise<void>(() => {
      try {
        JSON.parse("{bad");
      } catch (e) {
        console.log("2 js catch inside inner executor:", (e as Error).name);
      }
      throw new Error("inner executor");
    }).catch((e) => console.log("2 inner rejection:", (e as Error).message));
    throw new Error("outer js");
  } catch (e) {
    console.log("2 js catch inside outer executor:", (e as Error).message);
  }
  reject(new Error("outer executor"));
}).catch((e) => console.log("2 outer rejection:", (e as Error).message));

// 3. A throw during a generator step inside for-of, and the cleanup paths.
function* steps() {
  try {
    yield 1;
    yield 2;
    throw new Error("generator step");
  } finally {
    console.log("3 generator finally");
  }
}
try {
  for (const v of steps()) console.log("3 value", v);
} catch (e) {
  console.log("3 for-of caught:", (e as Error).message);
}
function* closing() {
  try {
    yield "a";
    yield "b";
  } finally {
    console.log("3 closed by break");
  }
}
for (const v of closing()) {
  console.log("3 got", v);
  break;
}
try {
  const all = [...steps()];
  console.log("3 unreachable", all.length);
} catch (e) {
  console.log("3 spread caught:", (e as Error).message);
}
const g = steps();
g.next();
try {
  g.throw(new Error("thrown in"));
} catch (e) {
  console.log("3 g.throw caught:", (e as Error).message);
}

// 4. Iterator factory throws inside a runtime spread.
const badIterable = {
  [Symbol.iterator]() {
    throw new Error("factory");
  },
};
try {
  Array.from(badIterable as unknown as Iterable<number>);
} catch (e) {
  console.log("4 Array.from caught:", (e as Error).message);
}

// 5. A throw from a reaction, then catches keep working in the same drain.
Promise.resolve(1)
  .then(() => {
    throw new Error("reaction");
  })
  .catch((e) => {
    console.log("5 reaction caught:", (e as Error).message);
    try {
      deep(50);
    } catch (e2) {
      console.log("5 js catch in a reaction:", (e2 as Error).message);
    }
  });

// 6. Timer drain: a throw reaches uncaughtException, and the next timer runs.
process.on("uncaughtException", (e) => {
  console.log("6 uncaughtException:", (e as Error).message);
});
setTimeout(() => {
  throw new Error("timer");
}, 0);
setTimeout(() => console.log("6 next timer ran"), 5);

// 7. Catch order across many traps in one synchronous loop.
let caught = 0;
for (let i = 0; i < 2000; i++) {
  new Promise(() => {
    if (i % 2 === 0) throw new Error("x");
  }).catch(() => {
    caught++;
  });
}
setTimeout(() => console.log("7 caught", caught), 10);
