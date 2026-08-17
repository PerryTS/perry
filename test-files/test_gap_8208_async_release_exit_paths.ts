// #8208 (#7933 follow-up): every terminal exit path of a plain-async activation
// releases its box cells at most once, and only when nothing can still reach
// them. The release is invisible behaviourally — a cell freed while still
// reachable, or one cell handed to two live activations, shows up as a WRONG
// ANSWER, not a crash — so this fixture drives each exit-path shape hundreds of
// times and prints values that only come out right if every cell survived
// exactly as long as it should have.
//
// Shapes covered, one per function below: normal return; throw after an await
// (the reject terminal arm); early return from inside a loop after a suspend;
// await on a rejected promise; try/finally across a suspend, exercising the
// finally on BOTH terminal arms; loop-created closures capturing a
// per-iteration binding across a suspend (CLAUDE.md lists collapsing
// per-iteration bindings as a known async-to-generator weak spot, and the
// release sits directly on top of it); and async-generator early .return()
// versus a full drain.
//
// The counter-level gate for the same change lives in perry-runtime's
// `release_tests` (residue bounded, parked-cell inertness, flush-gated reuse);
// this one is the behavioural half.
const N = 400;
const log: string[] = [];

// 1. normal return
async function normalReturn(i: number): Promise<number> {
  const a = await Promise.resolve(i);
  const b = await Promise.resolve(a * 2);
  return a + b;
}

// 2. throw after an await (reject terminal arm)
async function throwsAfterAwait(i: number): Promise<number> {
  const a = await Promise.resolve(i);
  if (a % 3 === 0) throw new Error(`boom:${a}`);
  return a;
}

// 3. early return from inside a loop, after a suspend
async function earlyReturnInLoop(i: number): Promise<number> {
  let acc = 0;
  for (let k = 0; k < 10; k++) {
    const v = await Promise.resolve(k);
    acc = acc + v;
    if (v === (i % 7)) return acc * 100 + k;
  }
  return acc;
}

// 4. await on a promise that rejects (resumption via the reject path)
async function awaitRejected(i: number): Promise<string> {
  try {
    await Promise.reject(new Error(`rej:${i}`));
    return "unreachable";
  } catch (e) {
    const after = await Promise.resolve(i);
    return `caught ${(e as Error).message} then ${after}`;
  }
}

// 5. try/finally across a suspend — finally runs on BOTH terminal arms
async function finallyBothArms(i: number): Promise<string> {
  const marks: string[] = [];
  try {
    const a = await Promise.resolve(i);
    if (a % 5 === 0) throw new Error("f");
    marks.push(`ok${a}`);
    return marks.join(",");
  } catch {
    marks.push("err");
    return marks.join(",");
  } finally {
    marks.push("fin");
  }
}

// 6. loop-created closures capturing a per-iteration binding across a suspend.
// If two iterations ever share one cell (or a cell is released and reused
// while a closure still holds it) these print the wrong values.
async function loopClosures(i: number): Promise<Array<() => number>> {
  const fns: Array<() => number> = [];
  for (let k = 0; k < 6; k++) {
    const j = k * 10 + (i % 4);
    await Promise.resolve(k);
    fns.push(() => j);
  }
  return fns;
}

// 7. generator .return() / .throw() on an async generator
async function* agen(i: number): AsyncGenerator<number> {
  try {
    yield i;
    yield i + 1;
    yield i + 2;
  } finally {
    log.push(`agen-fin:${i % 5}`);
  }
}

async function main(): Promise<void> {
  let sum = 0;
  let errs = 0;
  const samples: string[] = [];

  for (let i = 0; i < N; i++) {
    sum = sum + (await normalReturn(i));

    try { sum = sum + (await throwsAfterAwait(i)); } catch { errs = errs + 1; }

    sum = sum + (await earlyReturnInLoop(i));

    const r = await awaitRejected(i);
    if (i === 3) samples.push(r);

    const f = await finallyBothArms(i);
    if (i === 5 || i === 6) samples.push(f);

    const retainedClosures = await loopClosures(i);
    if (i === 2) {
      // Keep the completed activation's captures alive across a real task-queue
      // drain, then allocate another async frame before reading them. Reusing a
      // closure-visible cell here would silently substitute the new frame's
      // value for the retained per-iteration binding.
      await new Promise<void>((resolve) => setTimeout(resolve, 0));
      await normalReturn(1000 + i);
      samples.push(retainedClosures.map((fn) => fn()).join("-"));
    }

    // async generator: early .return() on some iterations, full drain on others
    const g = agen(i);
    const first = await g.next();
    sum = sum + (first.value as number);
    if (i % 2 === 0) {
      await g.return(0 as unknown as number);
    } else {
      for await (const v of { [Symbol.asyncIterator]: () => g }) { sum = sum + v; }
    }
  }

  console.log(`sum=${sum}`);
  console.log(`errs=${errs}`);
  console.log(`samples=${samples.join(" | ")}`);
  console.log(`finmarks=${log.length}`);
}

main();
