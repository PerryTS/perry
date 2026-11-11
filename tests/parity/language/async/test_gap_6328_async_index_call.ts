// #6328: `await Promise.all([...])` silently evaporated (exit 0, no output) when
// the input promises were settled through executor-captured resolvers invoked
// from a loop.
//
// Root cause was not in the promise machinery at all: `arr[i](x)` — a call whose
// callee is a computed member with a *numeric* key — is routed to
// `js_native_call_method_value` whenever codegen cannot statically prove the key
// numeric. Inside an `async` function it never can (the async-to-generator
// transform turns body locals into boxed `Any` captures), and that runtime helper
// stringified the index and dispatched by METHOD NAME — which can never see an
// Array's element storage. The lookup missed, `undefined` came back, and the call
// was a silent no-op. The loop resolved nothing, so `await all` never resumed.
//
// The `for` loops below matter: at a low trip count the loop is unrolled to
// constant indices, which codegen *can* prove numeric — the bug only shows up
// once the index stays a runtime value.

async function combinator() {
  const resolvers: ((v: number) => void)[] = [];
  const ps = Array.from(
    { length: 100 },
    () => new Promise<number>((res) => { resolvers.push(res); }),
  );
  const all = Promise.all(ps);
  for (let i = 0; i < 100; i++) resolvers[i](i);
  const r = await all;
  console.log("all", r.length, r[0], r[99]);

  const settled = await Promise.allSettled(ps);
  console.log("allSettled", settled.length, settled[7].status);
  console.log("race", await Promise.race(ps));
  console.log("any", await Promise.any(ps));
}

// The underlying defect, with no promise anywhere: calling closures out of an
// array by runtime index, inside an async function.
async function indexCall() {
  const fns: ((v: number) => string)[] = [];
  for (let i = 0; i < 9; i++) fns.push((v) => "fn:" + v);
  const out: string[] = [];
  for (let i = 0; i < 9; i++) out.push(fns[i](i));
  await 0;
  console.log("indexCall", out.join(","));
}

// `this` must still be bound by the dynamic-key dispatch (#321): a numerically
// keyed own method on a plain object reads `this` through the same helper.
async function thisBinding() {
  const obj: any = { tag: "obj", 3: function () { return this.tag; } };
  let k = 3;
  console.log("thisBinding", obj[k]());
  await 0;
}

// A numerically named class method still resolves through the vtable — the
// element-read fast path must fall through when the key names no own value.
class Numbered {
  3() {
    return "vtable-3";
  }
}

async function vtable() {
  const c: any = new Numbered();
  let k = 3;
  console.log("vtable", c[k]());
  await 0;
}

// A rejecting member must still reject, and `await` must still see it.
async function rejects() {
  const resolvers: ((v: number) => void)[] = [];
  const rejecters: ((e: unknown) => void)[] = [];
  const ps = Array.from(
    { length: 10 },
    () =>
      new Promise<number>((res, rej) => {
        resolvers.push(res);
        rejecters.push(rej);
      }),
  );
  const all = Promise.all(ps);
  for (let i = 0; i < 9; i++) resolvers[i](i);
  rejecters[9](new Error("boom"));
  try {
    await all;
    console.log("rejects UNREACHABLE");
  } catch (e) {
    console.log("rejects", (e as Error).message);
  }
}

async function main() {
  await combinator();
  await indexCall();
  await thisBinding();
  await vtable();
  await rejects();
  console.log("done");
}

main();
