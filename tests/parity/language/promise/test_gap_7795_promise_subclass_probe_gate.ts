// #7795: `subclass_backing_promise` is now gated on a monotone
// `PROMISE_SUBCLASS_EVER` flag, so a program that never subclasses `Promise`
// stops paying a key-string allocation plus a full recursive
// `js_object_get_field_by_name` on every ordinary-object property MISS. That
// miss path is reached by the spec thenable check (`Get(v, "then")`) that runs
// on every `await`/resolve of a plain object, which made it the single hottest
// thing in an async service pipeline.
//
// A gate is only safe if the OPEN state is exercised: nothing in the tree used
// `class X extends Promise` before this file, so the probe had no coverage at
// all and a wrong gate would have silently broken Promise subclassing. This
// asserts the flag is armed at the stash site and the subclass still behaves.

class MyPromise<T> extends Promise<T> {
  tag(): string {
    return "mine";
  }
}

// Constructing the subclass is what arms the gate (the stash site).
const p = new MyPromise<number>((resolve) => {
  resolve(41);
});

console.log("is-mypromise", p instanceof MyPromise);
console.log("is-promise", p instanceof Promise);
console.log("tag", p.tag());

// The backing cell must still be reachable through the hidden field, i.e. the
// gated probe must return `Some` now that a subclass instance exists.
p.then((v: number) => {
  console.log("then", v + 1);
});

const r = MyPromise.resolve(7);
console.log("static-resolve-type", r instanceof Promise);
r.then((v: number) => console.log("static-then", v));

async function useIt(): Promise<number> {
  const v = await p;
  return v + 1;
}
useIt().then((v: number) => console.log("await", v));

// A plain (non-subclass) object must still resolve as a NON-thenable — this is
// the fast path the gate protects.
async function plain(): Promise<{ v: number }> {
  return { v: 5 };
}
plain().then((o: { v: number }) => console.log("plain-await", o.v));
