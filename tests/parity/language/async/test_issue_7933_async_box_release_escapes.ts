// #7933: a completed async activation releases (clears) its boxed body
// locals, but ONLY the ones no closure can observe. Every shape below keeps a
// closure alive past the activation that declared its local — if the release
// analysis widened to cover any of them, the reads would come back
// `undefined` and this file's output would change.
//
// Expected (node): 61 6 7 202 L:x 9 15 18 30 61 L:x

// 1. A closure returned from an async fn reads a local declared before the await.
async function returnsClosure(n: number): Promise<() => number> {
  const base = n * 10;
  await tick();
  const bump = base + 1;
  return () => base + bump;
}

// 2. A closure that WRITES a local declared before the await (mutable capture).
async function returnsWriter(n: number): Promise<() => number> {
  let acc = n;
  await tick();
  return () => {
    acc = acc + 1;
    return acc;
  };
}

// 3. A closure created BEFORE the await and returned after it.
async function closureBeforeAwait(n: number): Promise<() => number> {
  const seed = n + 100;
  const f = () => seed * 2;
  await tick();
  return f;
}

// 4. A closure stored into an object that outlives the activation.
type Holder = { get: () => string };
async function storesClosure(tag: string): Promise<Holder> {
  const label = `L:${tag}`;
  await tick();
  const h: Holder = { get: () => label };
  return h;
}

// 5. A closure in a nested async arrow reads the OUTER async fn's local.
async function nestedAsyncClosure(n: number): Promise<() => number> {
  const outer = n + 7;
  await tick();
  const inner = async () => {
    await tick();
    return outer;
  };
  await inner();
  return () => outer;
}

// 6. Locals captured by a closure created inside a try, after the await, with
//    the activation completing through the catch arm.
async function throwPathClosure(n: number): Promise<() => number> {
  let kept = n;
  try {
    await tick();
    kept = kept + 1;
    throw new Error("boom");
  } catch (e) {
    kept = kept + 10;
  }
  await tick();
  return () => kept;
}

// 7. A closure escaping through an array that outlives the activation.
async function closuresInLoop(n: number): Promise<Array<() => number>> {
  const out: Array<() => number> = [];
  for (let i = 0; i < n; i++) {
    const each = i * 3;
    await tick();
    out.push(() => each);
  }
  return out;
}

// 8. Rejection path: the local is read by a closure carried on the thrown
//    value. (A plain object, not an Error subclass — attaching a closure
//    property to an `Error` is a separate, pre-existing Perry gap.)
type Thrown = { peek: () => number };
async function rejectsWithClosure(n: number): Promise<number> {
  const secret = n * 5;
  await tick();
  const err: Thrown = { peek: () => secret };
  throw err;
}

function tick(): Promise<number> {
  return new Promise<number>((resolve) => {
    resolve(0);
  });
}


async function main(): Promise<void> {
  const parts: string[] = [];

  const c1 = await returnsClosure(3);
  parts.push(String(c1()));

  const c2 = await returnsWriter(5);
  parts.push(String(c2()));
  parts.push(String(c2()));

  const c3 = await closureBeforeAwait(1);
  parts.push(String(c3()));

  const h = await storesClosure("x");
  parts.push(h.get());

  const c5 = await nestedAsyncClosure(2);
  parts.push(String(c5()));

  const c6 = await throwPathClosure(4);
  parts.push(String(c6()));

  const arr = await closuresInLoop(4);
  let sum = 0;
  for (let i = 0; i < arr.length; i++) sum = sum + arr[i]();
  parts.push(String(sum));

  try {
    await rejectsWithClosure(6);
    parts.push("NOTHROWN");
  } catch (e) {
    const err = e as Thrown;
    parts.push(String(err.peek()));
  }

  // Force GC pressure so any released-but-still-reachable value would have
  // been collected (or would read `undefined`) by the time we print.
  const churn: number[][] = [];
  for (let i = 0; i < 20000; i++) {
    churn.push([i, i + 1, i + 2]);
    if (churn.length > 100) churn.length = 0;
  }
  parts.push(String(c1()));
  parts.push(h.get());

  console.log(parts.join(" "));
}

main();
