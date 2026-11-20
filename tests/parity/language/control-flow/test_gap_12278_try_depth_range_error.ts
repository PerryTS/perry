// A deeply recursive try/finally chain must throw a catchable RangeError
// rather than exhausting the runtime's try-handler storage.
function recurse(n: number): number {
  try {
    return recurse(n + 1);
  } finally {
    // Keep a finally handler active at every recursive depth.
  }
}

let caughtRangeError = false;
try {
  recurse(0);
} catch (error) {
  caughtRangeError = error instanceof RangeError;
}

if (!caughtRangeError) {
  throw new Error("deep recursion did not produce a catchable RangeError");
}

console.log("deep try/finally recursion produced a catchable RangeError");
