// Dynamic-bound counted loops whose bodies branch, call and nest: the integer
// copy must leave the counter's Number value exact on every way out.
function find(xs: any, target: any) {
  let i, len;
  for (i = 0, len = xs.length; i < len; i++) {
    if (xs[i] === target) break;
  }
  console.log("find", i, len);
}
find([3, 4, 5], 4);
find([3, 4, 5], 9);
find({ length: 2.5, 0: "a", 1: "b", 2: "c" }, "z");
find({ length: 2 ** 31 + 2, 0: 7 }, 7);
find({ length: "3", 0: 1, 1: 2, 2: 3 }, 3);

function walk(node: any, opts: any, depth: number): boolean {
  let i, ok, len;
  if (node.type === 5) {
    depth++;
    opts.reps++;
    if (depth > 1) return false;
  }
  const stack = node.stack;
  if (!stack) return true;
  for (i = 0, len = stack.length; i < len; i++) {
    ok = walk(stack[i], opts, depth);
    if (!ok) return false;
  }
  return true;
}
console.log("walk", walk({ stack: [{ type: 1 }, { type: 5, stack: [{ type: 1 }] }] }, { reps: 0 }, 0));
console.log("walk", walk({ stack: [{ type: 5, stack: [{ type: 5 }] }] }, { reps: 0 }, 0));

function firstIndex(xs: any, pred: (x: any) => boolean) {
  for (let i = 0, n = xs.length; i < n; i++) {
    if (pred(xs[i])) return i;
  }
  return -1;
}
console.log("firstIndex", firstIndex([1, 3, 6, 7], (x) => x % 2 === 0), firstIndex([], () => true));

function skipOdd(xs: any) {
  let i, n, sum = 0;
  for (i = 0, n = xs.length; i < n; i++) {
    if (xs[i] % 2) continue;
    switch (xs[i]) {
      case 4:
        sum += 100;
        break;
      default:
        sum += xs[i];
    }
  }
  console.log("skipOdd", sum, i);
}
skipOdd([1, 2, 3, 4, 5, 6]);

// An inner loop bounded by the outer counter reads it inside the integer copy.
function triangle(m: any) {
  let i, n, j, t = 0;
  for (i = 0, n = m; i < n; i++) {
    for (j = 0; j < i; j++) t += j;
    if (t > 50) break;
  }
  console.log("triangle", t, i, j);
}
triangle(5);
triangle(20);
triangle(4.5);

// Labelled exits: inner labels stay in the copy; outer ones bypass its exit.
function labels(rows: any) {
  let i, n, hits = 0;
  outer: for (let r = 0; r < rows.length; r++) {
    for (i = 0, n = rows[r].length; i < n; i++) {
      inner: for (let k = 0; k < 3; k++) {
        if (k === 1) continue inner;
        if (k === 2) break inner;
      }
      if (rows[r][i] < 0) continue outer;
      if (rows[r][i] === 0) break outer;
      hits++;
    }
  }
  console.log("labels", hits, i);
}
labels([[1, 2], [3, -1, 4], [5, 0, 6]]);

function caughtInside(xs: any) {
  let i, n, seen = "";
  for (i = 0, n = xs.length; i < n; i++) {
    try {
      if (xs[i] === null) throw new Error("x" + i);
      seen += xs[i];
    } catch (e: any) {
      seen += "[" + e.message + ":" + i + "]";
    } finally {
      seen += ";";
    }
  }
  console.log("caughtInside", seen, i);
}
caughtInside(["a", null, "b"]);

function thrower(xs: any) {
  for (let i = 0, n = xs.length; i < n; i++) {
    if (xs[i] === "boom") throw new Error("at " + i);
  }
}
try { thrower(["a", "boom"]); } catch (e: any) { console.log("thrower", e.message); }

// Negative controls stay correct: captured, written, bound written.
function captured(xs: any) {
  const fns: (() => number)[] = [];
  let i, n;
  for (i = 0, n = xs.length; i < n; i++) {
    if (xs[i]) fns.push(() => i);
  }
  console.log("captured", fns.map((f) => f()).join(","), i);
}
captured([1, 0, 1]);

function written(xs: any) {
  let i, n, s = 0;
  for (i = 0, n = xs.length; i < n; i++) {
    if (xs[i] === 2) i += 0.5;
    s += i;
  }
  console.log("written", s, i);
}
written([1, 2, 3, 4]);

function shrinking(xs: any) {
  let i, n, s = 0;
  for (i = 0, n = xs.length; i < n; i++) {
    if (i === 1) n = 2;
    s += xs[i];
  }
  console.log("shrinking", s, i);
}
shrinking([1, 2, 3, 4]);

function negativeStart(xs: any) {
  let i, n, s = "";
  for (i = -2, n = xs.length; i < n; i++) {
    if (i < 0) { s += "<"; continue; }
    s += xs[i];
  }
  console.log("negativeStart", s, i);
}
negativeStart("abc");
