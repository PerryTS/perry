// #2308 — a `var` declared inside a constant-bound (compile-time unrolled)
// counted loop and read AFTER the loop must hold the last iteration's value,
// per JS function-scoped `var` semantics. The unroller used to rename each
// unrolled copy's declaration to a fresh id, so the post-loop read bound to
// an id no copy ever wrote and observed the initial (0/undefined) value.
// Output is byte-for-byte vs `node --experimental-strip-types`.

// var read after a constant-bound (unrolled) loop
function lastDouble(): number {
  let sum = 0;
  for (let i = 0; i < 3; i++) {
    var t = i * 2;
    sum += t;
  }
  return sum + t; // sum 6 + t 4 = 10
}
console.log(lastDouble());

// var written in inner loop, read after the OUTER loop (nested unroll)
function nestedLast(): number {
  let acc = 0;
  for (let i = 0; i < 2; i++) {
    for (let j = 0; j < 2; j++) {
      var last = i * 10 + j;
      acc += 1;
    }
  }
  return last; // last assignment: i=1, j=1 -> 11
}
console.log(nestedLast());

// guard: a block-scoped `let` captured by a per-iteration closure must STILL
// get a distinct binding per unrolled copy (the unroller's original job).
function distinctCaptures(): number {
  const fns: (() => number)[] = [];
  for (let i = 0; i < 3; i++) {
    let x = i;
    fns.push(() => x);
  }
  return fns[0]() + fns[1]() + fns[2](); // 0 + 1 + 2 = 3
}
console.log(distinctCaptures());

// NOTE: a `var` assigned inside a *conditional* within an unrolled loop and
// read after the loop (e.g. `for (...) { if (i===2) { var hit = ... } }`)
// additionally depends on the per-branch `var` slot fix from #1803 — after
// unrolling it becomes the canonical #1803 shape (one `Let` per if-branch +
// a read past the merge). It is covered by test_ajv_standalone_typed.ts /
// #1803, not here, so this file stays parity-clean independent of #1803.
