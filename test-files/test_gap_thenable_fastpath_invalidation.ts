// #7910 — the promise-assimilation fast path (`Get(resolution, "then")`) must
// stay observationally identical to the spec path across every route that can
// put a `then` on a resolution value's [[Get]] chain.
//
// The shape of every case below is deliberate: WARM the same call site with
// plain objects first (so the fast negative is taken and, for the
// `Object.prototype` half, memoized), THEN mutate, THEN resolve again through
// the SAME call site. A fast path that fails to invalidate does not print a
// wrong value here — it HANGS, because the awaiting promise never settles. So
// every case also has a plain, non-hanging control line after it.

const log: string[] = [];

// One shared call site for every case. `resolveHere` is where the runtime's
// `Get(resolution, "then")` happens.
function resolveHere(v: unknown): Promise<unknown> {
  return Promise.resolve(v);
}

async function warm(rounds: number): Promise<number> {
  let n = 0;
  for (let i = 0; i < rounds; i++) {
    const r = (await resolveHere({ i })) as { i: number };
    n += r.i;
  }
  return n;
}

async function main() {
  log.push("warm=" + (await warm(200)));

  // ── 1. plain assignment onto Object.prototype ────────────────────────────
  (Object.prototype as any).then = function (res: (v: unknown) => void) {
    res("assign");
  };
  log.push("assign -> " + (await resolveHere({ tag: 1 })));
  delete (Object.prototype as any).then;
  log.push("assign-removed -> " + JSON.stringify(await resolveHere({ tag: 1 })));

  // ── 2. Object.defineProperty, data descriptor ────────────────────────────
  await warm(50);
  Object.defineProperty(Object.prototype, "then", {
    value: function (res: (v: unknown) => void) {
      res("defineProperty-data");
    },
    configurable: true,
    writable: true,
  });
  log.push("dp-data -> " + (await resolveHere({ tag: 2 })));
  delete (Object.prototype as any).then;
  log.push("dp-data-removed -> " + JSON.stringify(await resolveHere({ tag: 2 })));

  // ── 3. Object.defineProperty, ACCESSOR descriptor ────────────────────────
  // The getter is observable: it must run on EVERY resolution, so a memoized
  // "no then" verdict would show up as a call count that is too low.
  await warm(50);
  let getterCalls = 0;
  Object.defineProperty(Object.prototype, "then", {
    get() {
      getterCalls++;
      return function (res: (v: unknown) => void) {
        res("accessor");
      };
    },
    configurable: true,
  });
  log.push("accessor -> " + (await resolveHere({ tag: 3 })));
  log.push("accessor -> " + (await resolveHere({ tag: 3 })));
  delete (Object.prototype as any).then;
  log.push("accessor-getter-called=" + (getterCalls > 0));
  log.push("accessor-removed -> " + JSON.stringify(await resolveHere({ tag: 3 })));

  // ── 4. an accessor whose getter answers `undefined` ──────────────────────
  // Reads as "no then", but is still observable — the getter must keep being
  // invoked rather than being memoized away.
  await warm(50);
  let undefGetterCalls = 0;
  Object.defineProperty(Object.prototype, "then", {
    get() {
      undefGetterCalls++;
      return undefined;
    },
    configurable: true,
  });
  const u1 = await resolveHere({ tag: 4 });
  const u2 = await resolveHere({ tag: 4 });
  const u3 = await resolveHere({ tag: 4 });
  const sawCalls = undefGetterCalls;
  delete (Object.prototype as any).then;
  log.push("undef-accessor -> " + JSON.stringify([u1, u2, u3]));
  log.push("undef-accessor-calls>=3 -> " + (sawCalls >= 3));

  // ── 5. Object.assign onto Object.prototype ───────────────────────────────
  await warm(50);
  Object.assign(Object.prototype, {
    then(res: (v: unknown) => void) {
      res("object-assign");
    },
  });
  log.push("object-assign -> " + (await resolveHere({ tag: 5 })));
  delete (Object.prototype as any).then;
  log.push("object-assign-removed -> " + JSON.stringify(await resolveHere({ tag: 5 })));

  // ── 6. Reflect.set / Reflect.defineProperty ──────────────────────────────
  await warm(50);
  Reflect.set(Object.prototype, "then", function (res: (v: unknown) => void) {
    res("reflect-set");
  });
  log.push("reflect-set -> " + (await resolveHere({ tag: 6 })));
  delete (Object.prototype as any).then;

  await warm(50);
  Reflect.defineProperty(Object.prototype, "then", {
    value: function (res: (v: unknown) => void) {
      res("reflect-dp");
    },
    configurable: true,
    writable: true,
  });
  log.push("reflect-dp -> " + (await resolveHere({ tag: 6 })));
  delete (Object.prototype as any).then;
  log.push("reflect-removed -> " + JSON.stringify(await resolveHere({ tag: 6 })));

  // ── 7. a real thenable: `then` on the object ITSELF ──────────────────────
  // Never cached by any fast path — must always be found.
  await warm(50);
  log.push(
    "own-then -> " +
      (await resolveHere({
        then(res: (v: unknown) => void) {
          res("own-then");
        },
      })),
  );
  const late: any = { tag: 7 };
  log.push("own-then-added-later -> " + JSON.stringify(await resolveHere(late)));
  late.then = function (res: (v: unknown) => void) {
    res("own-then-late");
  };
  log.push("own-then-added-later -> " + (await resolveHere(late)));

  // ── 8. an own ACCESSOR `then` on the object itself ───────────────────────
  await warm(50);
  const acc: any = { tag: 8 };
  Object.defineProperty(acc, "then", {
    get() {
      return function (res: (v: unknown) => void) {
        res("own-accessor");
      };
    },
    configurable: true,
  });
  log.push("own-accessor -> " + (await resolveHere(acc)));

  // ── 9. setPrototypeOf / __proto__ on the object itself ───────────────────
  await warm(50);
  const viaProto: any = { tag: 9 };
  Object.setPrototypeOf(viaProto, {
    then(res: (v: unknown) => void) {
      res("set-prototype-of");
    },
  });
  log.push("setPrototypeOf -> " + (await resolveHere(viaProto)));

  await warm(50);
  const viaDunder: any = { tag: 9 };
  viaDunder.__proto__ = {
    then(res: (v: unknown) => void) {
      res("dunder-proto");
    },
  };
  log.push("__proto__ -> " + (await resolveHere(viaDunder)));

  // ── 10. a null-prototype object ──────────────────────────────────────────
  await warm(50);
  const bare = Object.create(null);
  bare.tag = 10;
  log.push("null-proto -> " + JSON.stringify(await resolveHere(bare)));
  const bareThen = Object.create(null);
  bareThen.then = function (res: (v: unknown) => void) {
    res("null-proto-then");
  };
  log.push("null-proto-then -> " + (await resolveHere(bareThen)));

  // ── 11. Object.setPrototypeOf(Object.prototype, …) ───────────────────────
  // `Object.prototype` is an immutable-prototype exotic object (ECMA-262
  // 10.4.7), so this route is closed by the spec itself — but the fast path
  // must not depend on that, and the throw must still reach user code. A `then`
  // two links up is otherwise reachable via a normal intermediate prototype,
  // which case 9 already covers.
  await warm(50);
  const grand = { then(res: (v: unknown) => void) { res("grandparent"); } };
  let protoOfProto = "no-throw";
  try {
    Object.setPrototypeOf(Object.prototype, grand);
  } catch (e) {
    protoOfProto = "throws:" + (e instanceof TypeError);
  }
  log.push("proto-of-proto -> " + protoOfProto);
  log.push("proto-of-proto-after -> " + JSON.stringify(await resolveHere({ tag: 11 })));

  // A `then` two links up through an ordinary intermediate prototype.
  await warm(50);
  const mid = Object.create({
    then(res: (v: unknown) => void) {
      res("grandparent");
    },
  });
  const deep: any = Object.create(mid);
  deep.tag = 11;
  log.push("inherited-two-levels -> " + (await resolveHere(deep)));

  // ── 12. await, not just Promise.resolve ──────────────────────────────────
  // `await` reaches the probe through a different runtime entry point
  // (`js_assimilate_thenable`) than `Promise.resolve` (`get_then_action`).
  await warm(50);
  (Object.prototype as any).then = function (res: (v: unknown) => void) {
    res("await-path");
  };
  const awaited = await ({ tag: 12 } as any);
  delete (Object.prototype as any).then;
  log.push("await-path -> " + awaited);

  // ── 13. an async function RETURNING a plain object ───────────────────────
  // Its return value is assimilated by the resolve path, not by an `await`.
  await warm(50);
  const returner = async () => ({ tag: 13 });
  log.push("returned -> " + JSON.stringify(await returner()));
  (Object.prototype as any).then = function (res: (v: unknown) => void) {
    res("returned-assimilated");
  };
  const r13 = await returner();
  delete (Object.prototype as any).then;
  log.push("returned -> " + r13);

  log.push("done");
  console.log(log.join("\n"));
}

main();
