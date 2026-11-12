// A promise resolve function adopts a thenable (ECMA-262 27.2.1.3.2 steps
// 9-15, 27.2.2.2 NewPromiseResolveThenableJob): the thenable's `then` is
// called with fresh resolve functions and the promise settles with what they
// are given, never with the thenable. In the whole-program solve the resolve
// function copied its argument as it is, so a `then` reached only through it
// (the executor's resolve, a kept resolve, a thenable that resolves a
// thenable) was called inactive while it ran (check mode
// `PERRY_VERIFY_FACTS=1`: "inactive body: body entered"; with
// `PERRY_FACTS_DCE=1` the removed body trapped). Negative controls: values
// whose `then` is no function fulfil as themselves.
const log: string[] = [];

async function main(): Promise<void> {
  // The executor's resolve, a method `then`.
  await new Promise((res: any) => res({ then(r: any) { log.push("then-1"); r(1); } })).then(
    (v: any) => log.push("v1=" + v),
  );

  // A resolve function kept and called later.
  let keep: any;
  const p2 = new Promise((r: any) => {
    keep = r;
  });
  keep({ then(r: any) { log.push("then-2"); r(2); } });
  await p2.then((v: any) => log.push("v2=" + v));

  // A thenable's `then` resolves another thenable.
  await new Promise((res: any) =>
    res({ then(r: any) { log.push("then-3a"); r({ then(r2: any) { log.push("then-3b"); r2(3); } }); } }),
  ).then((v: any) => log.push("v3=" + v));

  // `await` of a promise its executor resolved with a thenable.
  const v4 = await new Promise((res: any) => res({ then(r: any) { log.push("then-4"); r(4); } }));
  log.push("v4=" + v4);

  // What the thenable resolves flows on: a function, then called.
  await new Promise((res: any) => res({ then(r: any) { r(() => "fn-5"); } })).then((f: any) =>
    log.push("v5=" + f()),
  );

  // A thenable that rejects, and one whose `then` throws.
  await new Promise((res: any) => res({ then(_r: any, j: any) { j(new Error("e6")); } })).catch(
    (e: any) => log.push("c6=" + e.message),
  );
  await new Promise((res: any) =>
    res({ then() { throw new Error("e7"); } }),
  ).catch((e: any) => log.push("c7=" + e.message));

  // A native promise given to the executor's resolve.
  await new Promise((res: any) => res(Promise.resolve(8))).then((v: any) => log.push("v8=" + v));

  // A class instance with a `then` method.
  class Lazy {
    n: number;
    constructor(n: number) {
      this.n = n;
    }
    then(r: any) {
      log.push("then-9");
      r(this.n * 10);
    }
  }
  await new Promise((res: any) => res(new Lazy(9))).then((v: any) => log.push("v9=" + v));

  // Negative controls: `then` that is no function fulfils as itself.
  await new Promise((res: any) => res({ then: 5, k() { return "k10"; } })).then((o: any) =>
    log.push("v10=" + o.k() + "/" + o.then),
  );
  await new Promise((res: any) => res({ k() { return "k11"; } })).then((o: any) =>
    log.push("v11=" + o.k()),
  );
  await new Promise((res: any) =>
    res({
      get then() {
        log.push("get-12");
        return undefined;
      },
      k: "k12",
    }),
  ).then((o: any) => log.push("v12=" + o.k));
  await new Promise((res: any) => res("str13")).then((v: any) => log.push("v13=" + v));
}

main().then(() => {
  for (const l of log) console.log(l);
});
