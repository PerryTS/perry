// #7911: `Promise.all` runs a guarded per-element fast arm when the
// combinator is on the intrinsic `Promise` with an unpatched
// `Promise.resolve` and the element resolves to a plain native promise.
//
// This file pins the observables of the cases that DO take that arm: the
// microtask job graph (tick counts and interleaving), rejection ordering and
// unhandled-rejection accounting, and the shape/identity of the resolved
// values. Every line here must stay byte-identical to
// `node --experimental-strip-types`.
//
// The complementary file, `test_gap_7911_promise_all_slow_arm_guards.ts`,
// pins the cases that must FALL BACK to the generic spec path.

function say(s: string) {
  console.log(s);
}

// Race the subject against a chain of plain microtask ticks: the number of
// ticks that elapse before it settles is the combinator's job count, and is
// the single most sensitive thing a `then`-bypassing fast path can break.
function tickRace(label: string, make: () => Promise<unknown>): Promise<void> {
  return new Promise<void>((done) => {
    let ticks = 0;
    let settled = false;
    const bump = () => {
      if (settled) return;
      ticks++;
      if (ticks > 40) {
        say(label + " ticks=OVERFLOW");
        done();
        return;
      }
      Promise.resolve().then(bump);
    };
    make().then(() => {
      settled = true;
      say(label + " ticks=" + ticks);
      done();
    });
    Promise.resolve().then(bump);
  });
}

async function main() {
  // ---- microtask job counts -------------------------------------------
  await tickRace("a empty", () => Promise.all([]));
  await tickRace("b one-plain", () => Promise.all([1]));
  await tickRace("c three-plain", () => Promise.all([1, 2, 3]));
  await tickRace("d one-promise", () => Promise.all([Promise.resolve(1)]));
  await tickRace("e two-promises", () =>
    Promise.all([Promise.resolve(1), Promise.resolve(2)]),
  );
  await tickRace("f thenable", () => Promise.all([{ then: (r: any) => r(1) } as any]));
  await tickRace("g nested-resolve", () =>
    Promise.all([Promise.resolve(Promise.resolve(1))]),
  );

  // ---- interleaving against an independent chain ------------------------
  const seq: string[] = [];
  await new Promise<void>((done) => {
    Promise.resolve()
      .then(() => seq.push("a1"))
      .then(() => seq.push("a2"))
      .then(() => seq.push("a3"))
      .then(() => seq.push("a4"))
      .then(() => seq.push("a5"))
      .then(() => {
        say("h " + seq.join(","));
        done();
      });
    Promise.all([1, 2]).then(() => seq.push("ALL"));
  });

  // ---- rejection ordering and unhandled accounting ----------------------
  let unhandled = 0;
  const onUnhandled = () => {
    unhandled++;
  };
  (process as any).on("unhandledRejection", onUnhandled);

  // The first rejection wins; the later one must be swallowed AND must not
  // surface as an unhandled rejection. A fast path that skips `then` never
  // marks the loser handled and kills the process here.
  let which = "none";
  try {
    await Promise.all([Promise.reject(new Error("A")), Promise.reject(new Error("B"))]);
  } catch (e: any) {
    which = e.message;
  }
  await new Promise((r) => setTimeout(r, 20));
  say("i first-rejection=" + which + " unhandled=" + unhandled);

  unhandled = 0;
  try {
    await Promise.all([Promise.reject(new Error("x")), Promise.resolve(1)]);
  } catch {
    /* handled */
  }
  await new Promise((r) => setTimeout(r, 20));
  say("j unhandled=" + unhandled);
  (process as any).off("unhandledRejection", onUnhandled);

  // Timer-ordered rejections: the earlier one wins.
  const p1 = new Promise((_, rej) => setTimeout(() => rej(new Error("r1")), 0));
  const p2 = new Promise((_, rej) => setTimeout(() => rej(new Error("r2")), 5));
  let caught = "none";
  try {
    await Promise.all([p1, p2]);
  } catch (e: any) {
    caught = e.message;
  }
  await new Promise((r) => setTimeout(r, 20));
  say("k " + caught);

  // ---- values and shape -------------------------------------------------
  const obj = { a: 1 };
  const mixed = await Promise.all([1, "two", true, null, undefined, obj, NaN, -0, 10n] as any);
  say(
    "l " +
      mixed
        .map((v: any) =>
          typeof v === "bigint" ? v.toString() + "n" : Object.is(v, -0) ? "-0" : String(v),
        )
        .join("|"),
  );
  say("m sameObj=" + (mixed[5] === obj));

  const shape = await Promise.all([1, 2, 3]);
  say(
    "n isArray=" +
      Array.isArray(shape) +
      " len=" +
      shape.length +
      " proto=" +
      (Object.getPrototypeOf(shape) === Array.prototype) +
      " keys=" +
      Object.keys(shape).join(","),
  );

  // Index order, not settlement order.
  const slow = new Promise((res) => setTimeout(() => res("slow"), 10));
  say("o " + JSON.stringify(await Promise.all([slow, Promise.resolve("fast"), "plain"])));

  // Pending elements resolved out of order.
  const pend: Promise<number>[] = [];
  const resolvers: any[] = [];
  for (let i = 0; i < 4; i++) pend.push(new Promise<number>((res) => resolvers.push(res)));
  resolvers[2](2);
  resolvers[0](0);
  resolvers[3](3);
  resolvers[1](1);
  say("p " + JSON.stringify(await Promise.all(pend)));

  // The SAME pending promise three times: three independent reactions on one
  // promise, so the reaction-slot overflow path is exercised.
  let rres: any;
  const one = new Promise((res) => (rres = res));
  const three = Promise.all([one, one, one]);
  rres("same");
  say("q " + JSON.stringify(await three));

  // A user reaction already attached to the element: both must fire, in
  // registration order.
  const seq2: string[] = [];
  const shared = Promise.resolve("v");
  shared.then(() => seq2.push("user"));
  await Promise.all([shared]).then(() => seq2.push("all"));
  say("r " + seq2.join(","));

  // Duplicates, nesting, other iterables, async-function elements.
  const dup = Promise.resolve("d");
  say("s " + JSON.stringify(await Promise.all([dup, dup, dup])));
  say("t " + JSON.stringify(await Promise.all([Promise.resolve(Promise.resolve("inner"))])));
  say("u " + JSON.stringify(await Promise.all([Promise.all([1, 2]), Promise.all([3, 4])])));
  say("v " + JSON.stringify(await Promise.all(new Set([1, 2, 2, 3]))));
  say("w " + JSON.stringify(await Promise.all("abc" as any)));

  async function unit(n: number): Promise<number> {
    return n * 3;
  }
  const jobs: Promise<number>[] = [];
  for (let i = 0; i < 8; i++) jobs.push(unit(i));
  say("x " + JSON.stringify(await Promise.all(jobs)));

  // Scale: 1000 native promises through one call.
  const big: Promise<number>[] = [];
  for (let i = 0; i < 1000; i++) big.push(Promise.resolve(i));
  const rb = await Promise.all(big);
  let sum = 0;
  for (const v of rb) sum += v;
  say("y len=" + rb.length + " sum=" + sum);

  // A promise resolved with itself rejects with a TypeError, and that
  // rejection must be handled by the combinator's reject reaction.
  let selfErr = "none";
  {
    let selfRes: any;
    const self = new Promise((res) => (selfRes = res));
    selfRes(self);
    try {
      await Promise.all([self]);
    } catch (e: any) {
      selfErr = e.constructor.name;
    }
  }
  say("z " + selfErr);
}

main();
