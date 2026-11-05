// #2383 — `break <label>` targeting a labeled *block* statement (not a loop).
// `a: { ... break a; ... }` is valid JS/TS that exits the block; Perry only
// supported labeled break/continue against labeled *loops* and rejected this
// with "labeled break 'a' outside any loop". The pattern is the hard blocker
// for compiling React (and thus ink, #348): minified react.production.js /
// react.development.js and react-reconciler all use `a: { … break a; }`.
// Fix lowers a labeled block to a labeled run-once `do { … } while (false)`.
// Output is byte-for-byte vs `node --experimental-strip-types`.

// Basic labeled block: break exits past the rest of the block.
function f(x: number): number {
  let r = 0;
  a: {
    if (x > 0) {
      r = 1;
      break a;
    }
    r = 2;
  }
  return r;
}
console.log(f(5), f(-1)); // 1 2

// Nested labeled blocks: inner break vs. outer break.
function g(x: number): string {
  let out = "";
  outer: {
    out += "A";
    inner: {
      out += "B";
      if (x === 1) break inner; // skip "C", continue at "D"
      if (x === 2) break outer; // skip "C" and "D", continue at "E"
      out += "C";
    }
    out += "D";
  }
  out += "E";
  return out;
}
console.log(g(0), g(1), g(2)); // ABCDE ABDE ABE

// Labeled block whose break is never taken — body falls through normally.
function h(): number {
  let n = 0;
  blk: {
    n += 10;
    n += 5;
  }
  return n;
}
console.log(h()); // 15

// Labeled block containing a loop; break <block-label> from inside the loop
// must exit the whole block, not just the loop.
function k(): number {
  let sum = 0;
  done: {
    for (let i = 0; i < 10; i++) {
      sum += i;
      if (sum >= 6) break done;
    }
    sum = -1; // unreachable once break done fires
  }
  return sum;
}
console.log(k()); // 6

// Module-level labeled block.
let m = 0;
top: {
  m = 1;
  if (m === 1) break top;
  m = 99;
}
console.log(m); // 1
