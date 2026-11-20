// #7911: the cases `Promise.all`'s per-element fast arm must REFUSE.
//
// The fast arm replaces two observable spec steps —
// `Call(promiseResolve, C, «next»)` and `Invoke(nextPromise, "then", …)` —
// with the runtime primitives they select. That is only sound when the
// combinator is on the intrinsic `Promise`, `Promise.resolve` is the
// intrinsic, and the resolved element is a plain native promise with no own
// `then` and no own `constructor`. Each case below breaks exactly one of
// those and must still behave like `node --experimental-strip-types`.

function say(s: string) {
  console.log(s);
}
async function step(name: string, fn: () => Promise<void>) {
  try {
    await fn();
  } catch (e: any) {
    say(name + " THREW " + (e && e.constructor ? e.constructor.name : "?") + ": " + (e && e.message));
  }
}

const origResolve = Promise.resolve;

async function main() {
  // --- `Promise.resolve` is read ONCE off the constructor ----------------
  await step("a", async () => {
    let getCount = 0;
    Object.defineProperty(Promise, "resolve", {
      configurable: true,
      get() {
        getCount++;
        return origResolve;
      },
    });
    await Promise.all([1, 2, 3]);
    Object.defineProperty(Promise, "resolve", {
      configurable: true,
      writable: true,
      value: origResolve,
    });
    say("a getCount=" + getCount);
  });

  // --- and CALLED per element, with `this` = the constructor -------------
  await step("b", async () => {
    const calls: string[] = [];
    (Promise as any).resolve = function (v: any) {
      calls.push(String(v) + ":" + (this === Promise));
      return origResolve.call(this, v);
    };
    const r = await Promise.all([1, 2]);
    (Promise as any).resolve = origResolve;
    say("b " + calls.join(",") + " r=" + JSON.stringify(r));
  });

  // --- a patched `Promise.resolve` TRANSFORMS the element values ---------
  await step("c", async () => {
    (Promise as any).resolve = function (v: any) {
      return origResolve.call(this, (v as number) * 10);
    };
    const r = await Promise.all([1, 2, 3]);
    (Promise as any).resolve = origResolve;
    say("c " + JSON.stringify(r));
  });

  // --- a non-callable `Promise.resolve` rejects with a TypeError ---------
  await step("d", async () => {
    (Promise as any).resolve = 42;
    let e = "none";
    try {
      await Promise.all([1]);
    } catch (err: any) {
      e = err.constructor.name;
    }
    (Promise as any).resolve = origResolve;
    say("d " + e);
  });

  // --- a throwing `Promise.resolve` propagates through IfAbruptReject ----
  await step("e", async () => {
    (Promise as any).resolve = function () {
      throw new Error("resolve-boom");
    };
    let e = "none";
    try {
      await Promise.all([1, 2]);
    } catch (err: any) {
      e = err.message;
    }
    (Promise as any).resolve = origResolve;
    say("e " + e);
  });

  // --- `Promise.resolve` observes promise identity (`Promise.resolve(p) === p`)
  await step("f", async () => {
    const p = origResolve.call(Promise, 7);
    let sawSame = false;
    (Promise as any).resolve = function (v: any) {
      if (v === p) sawSame = true;
      return origResolve.call(this, v);
    };
    await Promise.all([p]);
    (Promise as any).resolve = origResolve;
    say("f sawSame=" + sawSame);
  });

  // --- a non-constructor `this` still throws from NewPromiseCapability ---
  await step("g", async () => {
    let e = "none";
    try {
      await (Promise.all as any).call({}, []);
    } catch (err: any) {
      e = err.constructor.name;
    }
    say("g " + e);
  });

  // --- an OWN `then` on an element shadows the intrinsic ----------------
  await step("h", async () => {
    const p = origResolve.call(Promise, 1);
    let called = 0;
    (p as any).then = function (res: any) {
      called++;
      res("own");
    };
    const r = await Promise.all([p]);
    say("h called=" + called + " r=" + JSON.stringify(r));
  });

  // --- a non-callable own `then` is a TypeError -------------------------
  await step("i", async () => {
    const p = origResolve.call(Promise, 5);
    (p as any).then = 42;
    let e = "none";
    try {
      await Promise.all([p]);
    } catch (err: any) {
      e = err.constructor.name;
    }
    say("i " + e);
  });

  // --- plain thenables go through assimilation, not the native reaction --
  await step("j", async () => {
    say("j " + JSON.stringify(await Promise.all([{ then: (res: any) => res("T") } as any])));
  });

  // --- resolve-function-called-once, in all three shapes ----------------
  await step("k", async () => {
    const twice = {
      then(res: any) {
        res("first");
        res("second");
      },
    };
    const resThenRej = {
      then(res: any, rej: any) {
        res("ok");
        rej(new Error("late"));
      },
    };
    const throwAfter = {
      then(res: any) {
        res("done");
        throw new Error("after");
      },
    };
    say(
      "k " +
        JSON.stringify(await Promise.all([twice as any])) +
        JSON.stringify(await Promise.all([resThenRej as any])) +
        JSON.stringify(await Promise.all([throwAfter as any])),
    );
  });

  // --- a `then` that throws BEFORE resolving rejects the combinator -----
  await step("l", async () => {
    let e = "none";
    try {
      await Promise.all([
        {
          then() {
            throw new Error("before");
          },
        } as any,
      ]);
    } catch (err: any) {
      e = err.message;
    }
    say("l " + e);
  });

  // --- a thenable that stashes its resolve and calls it later ----------
  await step("m", async () => {
    let stash: any = null;
    const t = {
      then(res: any) {
        stash = res;
        res("first");
      },
    };
    const r = await Promise.all([t as any, "x"]);
    stash("late");
    await new Promise((rr) => setTimeout(rr, 5));
    say("m " + JSON.stringify(r));
  });

  // --- a custom constructor: capability construction is observable ------
  await step("n", async () => {
    let e = "none";
    class Bad {
      constructor(exec: any) {
        exec(
          () => {},
          () => {},
        );
        exec(
          () => {},
          () => {},
        );
      }
    }
    try {
      (Promise.all as any).call(Bad, []);
    } catch (err: any) {
      e = err.constructor.name;
    }
    let e2 = "none";
    class Never {
      constructor(_exec: any) {}
    }
    try {
      (Promise.all as any).call(Never, []);
    } catch (err: any) {
      e2 = err.constructor.name;
    }
    say("n " + e + " " + e2);
  });

  // --- a Promise subclass as the constructor ---------------------------
  await step("o", async () => {
    class P extends Promise<any> {}
    const r = await P.all([1, 2, 3]);
    const inst = P.all([1]);
    say("o " + JSON.stringify(r) + " instP=" + (inst instanceof P) + " instPromise=" + (inst instanceof Promise));
    await inst;
    say("o call " + JSON.stringify(await (Promise.all as any).call(P, [4, 5])));
  });

  // --- iterator protocol: the drain is not part of the fast arm ---------
  await step("p", async () => {
    const trace: string[] = [];
    const custom = {
      [Symbol.iterator]() {
        let i = 0;
        trace.push("iter");
        return {
          next() {
            trace.push("n" + i);
            if (i < 3) return { value: Promise.resolve(i++), done: false };
            return { value: undefined, done: true };
          },
        };
      },
    };
    const r = await Promise.all(custom as any);
    say("p " + trace.join(",") + " r=" + JSON.stringify(r));
  });

  await step("q", async () => {
    function* gen() {
      yield 10;
      yield Promise.resolve(20);
    }
    say("q " + JSON.stringify(await Promise.all(gen())));
  });

  await step("r", async () => {
    let e = "none";
    const bad = {
      [Symbol.iterator]() {
        let n = 0;
        return {
          next() {
            n++;
            if (n === 2) throw new Error("iter-boom");
            return { value: n, done: false };
          },
        };
      },
    };
    try {
      await Promise.all(bad as any);
    } catch (err: any) {
      e = err.message;
    }
    let e2 = "none";
    try {
      await Promise.all(5 as any);
    } catch (err: any) {
      e2 = err.constructor.name;
    }
    let e3 = "none";
    try {
      await (Promise.all as any)();
    } catch (err: any) {
      e3 = err.constructor.name;
    }
    say("r " + e + " " + e2 + " " + e3);
  });

  await step("s", async () => {
    const sparse = [1, , 3];
    const r = await Promise.all(sparse as any);
    say("s " + JSON.stringify(r) + " len=" + r.length);
    const empty = await Promise.all([]);
    say("s empty=" + JSON.stringify(empty) + " isArr=" + Array.isArray(empty));
  });
}

main();
