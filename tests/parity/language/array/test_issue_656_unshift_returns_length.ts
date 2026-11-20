// Issue #656: Array.prototype.unshift must return the new length, not the array.
// (The chained-property-receiver writeback bug — `b.items.unshift(...)` after
// realloc leaving the parent pointing at the stale header — is a separate
// pre-existing issue not covered here.)
const arr = [1, 2, 3];
const r = arr.unshift(0);
console.log("ret:", r);
console.log("after:", arr);

// Side-effect-only call (bare expression statement) still mutates correctly.
const arr4 = [5];
arr4.unshift(4);
console.log("after4:", arr4);

// Use return value as a numeric comparison (the issue's example pattern).
const queue: number[] = [];
const newLen = queue.unshift(42);
console.log("isNum:", typeof newLen === "number", "gt0:", newLen > 0);

// Multiple unshifts chained — return value reflects new length each time.
const arr5: number[] = [];
const r5a = arr5.unshift(3);
const r5b = arr5.unshift(2);
const r5c = arr5.unshift(1);
console.log("seq:", r5a, r5b, r5c);
console.log("after5:", arr5);
