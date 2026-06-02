// Regression test for #1824: `await` inside a labeled loop or a do-while loop.
//
// The async→generator transform splits the function body into a state machine
// at each `await`. Before the fix, labeled loops (`outer: for (...)`) and
// `do { ... } while (...)` loops were not recognized by the body walkers, so:
//   - their loop-body `let`s were never hoisted/boxed (lost across the await),
//   - and the loop itself was never linearized (the embedded `await` was not
//     split into a resume state).
// When the lost slot held an object, the resumed continuation dereferenced a
// garbage pointer (the SIGSEGV reported in #1824). With a number it silently
// produced a wrong result.
//
// Each case keeps a value alive across the await inside the loop and is byte
// compared against `node --experimental-strip-types`.

async function ident(x: number): Promise<number> {
  return x;
}

// 1) Labeled for-loop: `base` is computed before the await and read after,
//    in the nested expression `(total + base) + got`.
async function labeledFor(n: number): Promise<number> {
  let total = 0;
  outer: for (let i = 0; i < n; i++) {
    const base = i * 10;
    const got = await ident(i);
    total = total + base + got;
  }
  return total;
}

// 2) Labeled for-loop using `continue label` and `break label`.
async function labeledBreakContinue(n: number): Promise<number> {
  let total = 0;
  outer: for (let i = 0; i < n; i++) {
    const got = await ident(i);
    if (got === 2) continue outer; // skip adding 2
    if (got === 5) break outer; // stop before 5
    total = total + got;
  }
  return total;
}

// 3) do-while with an await and a cross-await accumulator.
async function doWhileSum(n: number): Promise<number> {
  let total = 0;
  let i = 0;
  do {
    const base = i * 10;
    const got = await ident(i);
    total = total + base + got;
    i++;
  } while (i < n);
  return total;
}

// 4) The #1824 shape: an object accumulator created before the await and
//    field-mutated after resume. A lost slot here is a garbage object pointer.
async function objectAccumulator(n: number): Promise<number> {
  const agg: { clicks: number }[] = [];
  for (let i = 0; i < n; i++) {
    const row = { clicks: i * 5 };
    const got = await ident(i);
    if (agg.length === 0) {
      agg.push({ clicks: row.clicks + got });
    } else {
      const existing = agg[0];
      existing.clicks = existing.clicks + row.clicks + got;
    }
  }
  return agg[0].clicks;
}

async function main(): Promise<void> {
  console.log("labeledFor:", await labeledFor(3)); // 0+(10+1)+(20+2)=33
  console.log("labeledBreakContinue:", await labeledBreakContinue(6)); // 0+1+3+4=8
  console.log("doWhileSum:", await doWhileSum(3)); // 33
  console.log("objectAccumulator:", await objectAccumulator(4)); // 0..3: 0+5+10+15 + 0+1+2+3 = 36
}

await main();
