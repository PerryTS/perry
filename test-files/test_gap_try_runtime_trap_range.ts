// ehfree 2b: a deep-recursion RangeError thrown through a runtime callback
// lands in the runtime trap's landing pad (or the JS try around it), with the
// stack unwound, and the program keeps running.

function rec(n: number): number {
  return rec(n + 1) + 1;
}

try {
  [0].forEach(() => rec(0));
} catch (e) {
  console.log("forEach:", e instanceof RangeError);
}

new Promise(() => rec(0)).catch((e) =>
  console.log("executor:", e instanceof RangeError),
);

Promise.resolve()
  .then(() => rec(0))
  .catch((e) => console.log("reaction:", e instanceof RangeError));

function* gen() {
  yield rec(0);
}
try {
  for (const v of gen()) console.log("unreachable", v);
} catch (e) {
  console.log("generator:", e instanceof RangeError);
}

setTimeout(() => {
  try {
    rec(0);
  } catch (e) {
    console.log("timer:", e instanceof RangeError);
  }
  console.log("done");
}, 0);
