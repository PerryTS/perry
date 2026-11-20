// #9539 — callbackify must keep synchronously-resolved object thenables and
// their queued promise reactions valid across nursery collections. The
// callbacks all complete before the observable output; the regression was an
// exit-time SIGSEGV when the final microtask checkpoint read a stale pointer.

import * as util from "node:util";

const N = 6000;

function churn(): number {
  const tmp: any[] = [];
  for (let k = 0; k < 480; k++) {
    tmp.push({ k, s: "t" + k, pad: [k, k + 1] });
  }
  return tmp.length;
}

function check(name: string, factory: (i: number) => any): void {
  let bad = 0;
  const notes: string[] = [];
  for (let i = 0; i < N; i++) {
    const c: any = factory(i);
    const want = "c" + i + ":480";
    let got: any;
    try {
      got = c.run();
    } catch (error: any) {
      got = "THREW:" + (error && error.message);
    }
    if (got !== want) {
      bad++;
      if (bad <= 2) notes.push("[" + i + " got=" + String(got) + "]");
    }
  }
  console.log(name + " bad=" + bad + notes.join(""));
}

function host(i: number, run: (this: any) => string): any {
  return { id: i, inner: { def: "c" + i }, run };
}

check("callbackify_object_thenable", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    const callbackified = util.callbackify(function () {
      return {
        then: function (resolve: any, _reject: any) {
          n = churn();
          resolve(1);
        },
      };
    } as any);
    callbackified(function () {});
    return this.inner.def + ":" + n;
  });
});
